use bevy::color::LinearRgba;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::dot_state::{has_dot_state, DotStateVisual};
use super::heal_impact::COMBATANT_BODY_RADIUS;
use super::spell_bolts::soft_dot_texture;
use super::warlock_dots::{dot_anchor, dot_stature};
use crate::states::play_match::components::*;

// ==============================================================================
// Immolate burn state — the 15 s DoT's flames on the victim
// ==============================================================================
//
// Immolate's apply moment already has its flame burst (`casting.rs` spawns
// `FlameParticle`s at the landing). Its DAMAGE-OVER-TIME STATE drew nothing:
// it was in neither the drip map nor the Warlock DoT detector, so a burning
// victim looked untouched for the whole 15 s. This module is that state.
//
// ## Client data (build 1.15.9.69547, `scripts/db2_spell_sweep.py`)
//
// All eight Immolate ranks collapse to SpellVisual 46. Its `(7,8)` aura-state
// event row is kit **235**, which attaches `spells/immolate_state_base.m2`
// (fdid 166403, SpellVisualEffectName 297) at attachment **19 = Base** — the
// FEET — at scale 1. The model has zero vertices: it is two additive particle
// emitters, both transcribed below.
//
// - **Flame licks** (emitter 0, `flamelicksmall.blp`, a 4×4 flame flipbook):
//   30/s from a 0.46-radius sphere at the feet, starting near-still (speed
//   variation 0.3) in a narrow upward cone and ACCELERATING upward (client
//   gravity −2.36), life 1.31 s — so each lick climbs ~2 yd, the full height
//   of the body. Deep red (176,34,34) → orange (227,78,30) → light orange
//   (249,124,60), alpha 0.31 → 1.0 → 0.22, half-size 0.26 → 0.60 → 0.29, the
//   middle key at 55 % of life.
// - **Crown embers** (emitter 1, `yellow_glow3a.blp`): 10/s off a 0.28-yd
//   plane at the top of the head, rising at 1.67 u/s with drag 0.35, life
//   0.4 s, stretched into streaks (tail length 1.05). Red (219,48,26) →
//   orange (238,111,32) → yellow (249,188,23), alpha 0.70 → 1.0 → 0, a
//   0.03 → 0.56 → 0.03 bloom, the middle key at 30 % of life.
//
// The kit also carries a SpellProceduralEffect (type 1, colour 0xE75641 =
// (231,86,65), value 0.2) — read as a faint orange body tint. NOT built: the
// body material is shared with stealth's alpha and the hit-reaction flash, and
// a tint channel is not this card's to open.
//
// ## Divergences (deliberate)
//
// 1. **Emission ring, not the client sphere.** The client emits within 0.46 of
//    the base; the body capsule here is 0.5 in radius, so a faithful sphere
//    would bury every lick inside the opaque body once it rose past the shins
//    (the AS-10 buried-inside-the-capsule lesson). Licks start on a ring
//    `FLAME_RING_INNER..FLAME_RING_OUTER` just outside the capsule instead.
// 2. **The flipbook is the soft radial sprite, stretched** by
//    `FLAME_ASPECT` vertically so a lick reads as a tongue, not a dot. Both
//    sprite kinds billboard about the vertical axis only, so the stretch stays
//    upright from any camera.
// 3. **Crown height.** The client's crown bone sits 1.84 above its base; the
//    capsule's crown is `CROWN_Y` above its transform.
//
// ## Channel budget
//
// This is a SUSTAINED on-victim channel, the scarce kind (project note:
// Curse of Tongues' state was declined as a fourth). It is placed to share as
// little as possible with the two Warlock states it can stack with: FIRE hue
// against Corruption's near-black murk and UA's violet, and the FEET/LEGS
// against their head/torso anchors. `IMMOLATE_FLAME_RATE_SCALE` is the blessed
// density knob if it reads as too much.
//
// House rules: spawn / animate / age / billboard / cleanup systems,
// `Res<Time>`, `AlphaMode::Add`, graphical-only registration in
// `states/mod.rs` (headless never runs any of it and stays byte-identical by
// construction), and no `game_rng` — all variation is hash-seeded.

/// Blessed master scale on both emitters' rates.
pub const IMMOLATE_FLAME_RATE_SCALE: f32 = 1.0;

/// Flame-lick emission rate (client emitter 0: 30/s).
pub const FLAME_RATE: f32 = 30.0;
/// Flame-lick life (client: 1.31 s).
pub const FLAME_LIFE: f32 = 1.31;
/// Upward acceleration of a lick (client gravity −2.36 u/s²).
pub const FLAME_RISE_ACCEL: f32 = 2.36;
/// Largest initial upward speed (client speed 0 with variation 0.3).
pub const FLAME_SPEED_VARIATION: f32 = 0.3;
/// Half-angle of the upward emission cone, radians (client vrange 0.113).
const FLAME_CONE: f32 = 0.113;
/// Emission ring radii. Must clear `COMBATANT_BODY_RADIUS` (see divergence 1).
pub const FLAME_RING_INNER: f32 = COMBATANT_BODY_RADIUS + 0.05;
pub const FLAME_RING_OUTER: f32 = COMBATANT_BODY_RADIUS + 0.25;
/// Height of attachment 19 (the feet) above a combatant's transform: the
/// capsule is centred on the transform, which stands 1.0 above the floor.
pub const FEET_Y: f32 = -1.0;
/// Half-size of a lick at start / middle / end of life (client scale track).
pub const FLAME_HALF_SIZE: [f32; 3] = [0.258, 0.603, 0.286];
/// Alpha at start / middle / end of life (client alpha track).
const FLAME_ALPHA: [f32; 3] = [0.306, 1.0, 0.216];
/// Where the middle key of the lick tracks sits in its life (client: 0.55).
const FLAME_MID: f32 = 0.55;
/// Deep red → orange → light orange (client colour track, RGB/255).
const FLAME_COLOR: [[f32; 3]; 3] = [
    [176.0 / 255.0, 34.0 / 255.0, 34.0 / 255.0],
    [227.0 / 255.0, 78.0 / 255.0, 30.0 / 255.0],
    [249.0 / 255.0, 124.0 / 255.0, 60.0 / 255.0],
];
/// Vertical stretch of a lick's sprite (divergence 2).
pub const FLAME_ASPECT: f32 = 1.6;
/// Emissive strength on the licks (lit-emissive so the bloom pass sees it).
const FLAME_EMISSIVE: f32 = 2.0;

/// Crown-ember emission rate (client emitter 1: 10/s).
pub const EMBER_RATE: f32 = 10.0;
/// Crown-ember life (client: 0.4 s).
pub const EMBER_LIFE: f32 = 0.4;
/// Crown-ember launch speed, straight up (client: 1.67 ± 0.13 u/s).
pub const EMBER_SPEED: f32 = 1.67;
const EMBER_SPEED_VARIATION: f32 = 0.13;
/// Fraction of velocity shed per second (client drag 0.35).
const EMBER_DRAG: f32 = 0.35;
/// The ember plane's side (client area 0.28 × 0.28).
const EMBER_AREA: f32 = 0.28;
/// Top of the capsule above the transform — attachment for the embers.
pub const CROWN_Y: f32 = 1.25;
const EMBER_HALF_SIZE: [f32; 3] = [0.031, 0.556, 0.028];
const EMBER_ALPHA: [f32; 3] = [0.698, 1.0, 0.0];
const EMBER_MID: f32 = 0.3;
/// Red → orange → yellow (client colour track, RGB/255).
const EMBER_COLOR: [[f32; 3]; 3] = [
    [219.0 / 255.0, 48.0 / 255.0, 26.0 / 255.0],
    [238.0 / 255.0, 111.0 / 255.0, 32.0 / 255.0],
    [249.0 / 255.0, 188.0 / 255.0, 23.0 / 255.0],
];
/// An ember's streak length relative to its width (client tail length 1.05
/// on a sprite already stretched along its motion).
const EMBER_STREAK: f32 = 2.5;
const EMBER_EMISSIVE: f32 = 2.4;

/// Never let a lick start below the floor (a pet's anchor can land there).
const FLOOR_CLEARANCE: f32 = 0.02;

/// One burning victim's emitter rig: a top-level entity parked at the
/// victim's feet, parent of every lick and ember it emits (so the fire moves
/// with a running victim). Lives exactly as long as an Immolate DoT routed to
/// [`DotStateVisual::ImmolateBurn`] is on a living victim.
#[derive(Component)]
pub struct ImmolateBurnRig {
    pub target: Entity,
    pub age: f32,
    pub flame_carry: f32,
    pub ember_carry: f32,
    pub emitted: u32,
    /// Shared across every sprite the rig emits.
    pub quad: Handle<Mesh>,
    pub sprite: Handle<Image>,
}

/// Which of the two client emitters a particle came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImmolateSparkKind {
    /// A flame lick climbing the body from the feet.
    Lick,
    /// A short streak off the crown.
    Ember,
}

/// One additive sprite of the burn, in its rig's local frame.
#[derive(Component)]
pub struct ImmolateSpark {
    pub kind: ImmolateSparkKind,
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    /// Pet stature scale (1.0 on a combatant).
    pub stature: f32,
}

/// 3-key piecewise-linear ramp with its middle key at `mid` of life.
pub fn ramp_keyed(track: [f32; 3], mid: f32, k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    if k < mid {
        track[0] + (track[1] - track[0]) * (k / mid)
    } else {
        track[1] + (track[2] - track[1]) * ((k - mid) / (1.0 - mid))
    }
}

fn ramp_rgb(track: [[f32; 3]; 3], mid: f32, k: f32) -> [f32; 3] {
    [0, 1, 2].map(|c| ramp_keyed([track[0][c], track[1][c], track[2][c]], mid, k))
}

/// A lick's or ember's half-size at life fraction `k` (before stature).
pub fn spark_half_size(kind: ImmolateSparkKind, k: f32) -> f32 {
    match kind {
        ImmolateSparkKind::Lick => ramp_keyed(FLAME_HALF_SIZE, FLAME_MID, k),
        ImmolateSparkKind::Ember => ramp_keyed(EMBER_HALF_SIZE, EMBER_MID, k),
    }
}

/// A lick's or ember's alpha at life fraction `k`.
pub fn spark_alpha(kind: ImmolateSparkKind, k: f32) -> f32 {
    match kind {
        ImmolateSparkKind::Lick => ramp_keyed(FLAME_ALPHA, FLAME_MID, k),
        ImmolateSparkKind::Ember => ramp_keyed(EMBER_ALPHA, EMBER_MID, k),
    }
}

fn spark_rgb(kind: ImmolateSparkKind, k: f32) -> [f32; 3] {
    match kind {
        ImmolateSparkKind::Lick => ramp_rgb(FLAME_COLOR, FLAME_MID, k),
        ImmolateSparkKind::Ember => ramp_rgb(EMBER_COLOR, EMBER_MID, k),
    }
}

fn spark_emissive(kind: ImmolateSparkKind) -> f32 {
    match kind {
        ImmolateSparkKind::Lick => FLAME_EMISSIVE,
        ImmolateSparkKind::Ember => EMBER_EMISSIVE,
    }
}

/// Width : height of a spark's sprite (both stretch upward).
fn spark_aspect(kind: ImmolateSparkKind) -> f32 {
    match kind {
        ImmolateSparkKind::Lick => FLAME_ASPECT,
        ImmolateSparkKind::Ember => EMBER_STREAK,
    }
}

/// Cheap deterministic jitter in [0, 1). Visual only — never `game_rng`.
fn burn_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let s = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    ((s >> 22) ^ s) as f32 / u32::MAX as f32
}

/// Lit-emissive additive material: `unlit` would discard the emissive the
/// bloom pass needs (the `warlock_dots.rs` round-2 finding), and `Add`
/// premultiplies the emissive by the base alpha, so the alpha ramp gates it.
fn spark_material(
    materials: &mut Assets<StandardMaterial>,
    kind: ImmolateSparkKind,
    k: f32,
    sprite: Handle<Image>,
) -> Handle<StandardMaterial> {
    let [r, g, b] = spark_rgb(kind, k);
    let e = spark_emissive(kind);
    materials.add(StandardMaterial {
        base_color: Color::srgba(r, g, b, spark_alpha(kind, k)),
        base_color_texture: Some(sprite.clone()),
        emissive: LinearRgba::rgb(r * e, g * e, b * e),
        emissive_texture: Some(sprite),
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    })
}

/// Shared handles, built once.
pub struct ImmolateAssets {
    quad: Handle<Mesh>,
    sprite: Handle<Image>,
}

/// Build a burn rig on every living victim whose Immolate DoT routes to
/// [`DotStateVisual::ImmolateBurn`] and has none yet. Keyed on the aura's
/// PRESENCE, so a refresh does not restart the fire; dead victims get
/// nothing (auras linger on corpses — the fear lesson).
pub fn spawn_immolate_burns(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<ImmolateAssets>>,
    victims: Query<(
        Entity,
        &Combatant,
        &Transform,
        Option<&ActiveAuras>,
        Option<&Pet>,
    )>,
    rigs: Query<&ImmolateBurnRig>,
) {
    use std::collections::HashSet;
    let burning: HashSet<Entity> = rigs.iter().map(|r| r.target).collect();
    for (entity, combatant, transform, auras, pet) in victims.iter() {
        if !combatant.is_alive()
            || burning.contains(&entity)
            || !has_dot_state(auras, DotStateVisual::ImmolateBurn)
        {
            continue;
        }
        let assets = assets.get_or_insert_with(|| ImmolateAssets {
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            sprite: images.add(soft_dot_texture()),
        });
        commands.spawn((
            ImmolateBurnRig {
                target: entity,
                age: 0.0,
                flame_carry: 0.0,
                ember_carry: 0.0,
                emitted: 0,
                quad: assets.quad.clone(),
                sprite: assets.sprite.clone(),
            },
            Transform::from_translation(feet_anchor(transform.translation, pet.is_some())),
            Visibility::default(),
            PlayMatchEntity,
        ));
    }
}

/// World position of attachment 19 (the feet) for a victim at `translation`,
/// kept off the floor.
pub fn feet_anchor(translation: Vec3, is_pet: bool) -> Vec3 {
    let at = dot_anchor(FEET_Y, translation, is_pet);
    at.with_y(at.y.max(FLOOR_CLEARANCE))
}

/// Follow the victim and emit both streams: licks on a ring just outside the
/// body at the feet, embers off a small plane at the crown.
pub fn animate_immolate_burns(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rigs: Query<(Entity, &mut ImmolateBurnRig, &mut Transform)>,
    targets: Query<(&Transform, Option<&Pet>), (With<Combatant>, Without<ImmolateBurnRig>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut rig, mut transform) in rigs.iter_mut() {
        rig.age += dt;
        let Ok((target, pet)) = targets.get(rig.target) else {
            continue;
        };
        let is_pet = pet.is_some();
        let feet = feet_anchor(target.translation, is_pet);
        transform.translation = feet;
        let stature = dot_stature(is_pet);
        // The crown, expressed in the rig's (feet-anchored) local frame.
        let crown = dot_anchor(CROWN_Y, target.translation, is_pet) - feet;

        rig.flame_carry += FLAME_RATE * IMMOLATE_FLAME_RATE_SCALE * dt;
        while rig.flame_carry >= 1.0 {
            rig.flame_carry -= 1.0;
            let seed = next_seed(entity, &mut rig);
            let theta = burn_jitter(seed) * std::f32::consts::TAU;
            let radius = FLAME_RING_INNER
                + (FLAME_RING_OUTER - FLAME_RING_INNER) * burn_jitter(seed ^ 0x51ED);
            let origin = Vec3::new(theta.cos(), 0.0, theta.sin()) * radius * stature;
            // A narrow upward cone, near-still at birth: the rise is the
            // acceleration's work (age_immolate_sparks).
            let tilt = FLAME_CONE * burn_jitter(seed ^ 0x27D4);
            let lean = Vec3::new(theta.cos(), 0.0, theta.sin()) * tilt.sin();
            let speed = FLAME_SPEED_VARIATION * burn_jitter(seed ^ 0x9E37);
            let velocity = (Vec3::Y * tilt.cos() + lean) * speed * stature;
            spawn_spark(
                &mut commands,
                &mut materials,
                &rig,
                entity,
                ImmolateSparkKind::Lick,
                origin,
                velocity,
                FLAME_LIFE,
                stature,
            );
        }

        rig.ember_carry += EMBER_RATE * IMMOLATE_FLAME_RATE_SCALE * dt;
        while rig.ember_carry >= 1.0 {
            rig.ember_carry -= 1.0;
            let seed = next_seed(entity, &mut rig);
            let origin = crown
                + Vec3::new(
                    (burn_jitter(seed) - 0.5) * EMBER_AREA,
                    0.0,
                    (burn_jitter(seed ^ 0x51ED) - 0.5) * EMBER_AREA,
                ) * stature;
            let speed =
                EMBER_SPEED + EMBER_SPEED_VARIATION * (burn_jitter(seed ^ 0x27D4) * 2.0 - 1.0);
            spawn_spark(
                &mut commands,
                &mut materials,
                &rig,
                entity,
                ImmolateSparkKind::Ember,
                origin,
                Vec3::Y * speed * stature,
                EMBER_LIFE,
                stature,
            );
        }
    }
}

fn next_seed(entity: Entity, rig: &mut ImmolateBurnRig) -> u32 {
    let i = rig.emitted;
    rig.emitted = rig.emitted.wrapping_add(1);
    entity.index().wrapping_add(i.wrapping_mul(0x9E37_79B9))
}

#[allow(clippy::too_many_arguments)]
fn spawn_spark(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    rig: &ImmolateBurnRig,
    rig_entity: Entity,
    kind: ImmolateSparkKind,
    origin: Vec3,
    velocity: Vec3,
    life: f32,
    stature: f32,
) {
    let material = spark_material(materials, kind, 0.0, rig.sprite.clone());
    let spark = commands
        .spawn((
            ImmolateSpark {
                kind,
                velocity,
                age: 0.0,
                life,
                stature,
            },
            Mesh3d(rig.quad.clone()),
            MeshMaterial3d(material),
            Transform::from_translation(origin).with_scale(spark_scale(kind, 0.0, stature)),
            NotShadowCaster,
        ))
        .id();
    commands.entity(rig_entity).add_child(spark);
}

/// A spark's quad scale at life fraction `k`: full size = 2 × half-size,
/// stretched upward by its aspect.
fn spark_scale(kind: ImmolateSparkKind, k: f32, stature: f32) -> Vec3 {
    let w = 2.0 * spark_half_size(kind, k) * stature;
    Vec3::new(w, w * spark_aspect(kind), 1.0).max(Vec3::splat(1e-4))
}

/// Move every spark (licks accelerate upward, embers shed speed to drag),
/// ramp its size, colour and alpha along the client tracks, and despawn it at
/// the end of its life.
pub fn age_immolate_sparks(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sparks: Query<(
        Entity,
        &mut ImmolateSpark,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let dt = time.delta_secs();
    for (entity, mut spark, mut transform, material) in sparks.iter_mut() {
        spark.age += dt;
        if spark.age >= spark.life {
            commands.entity(entity).despawn();
            continue;
        }
        match spark.kind {
            ImmolateSparkKind::Lick => {
                spark.velocity.y += FLAME_RISE_ACCEL * spark.stature * dt;
            }
            ImmolateSparkKind::Ember => {
                spark.velocity *= (1.0 - EMBER_DRAG * dt).max(0.0);
            }
        }
        transform.translation += spark.velocity * dt;
        let k = spark.age / spark.life;
        transform.scale = spark_scale(spark.kind, k, spark.stature);
        if let Some(m) = materials.get_mut(&material.0) {
            let [r, g, b] = spark_rgb(spark.kind, k);
            let e = spark_emissive(spark.kind);
            m.base_color = Color::srgba(r, g, b, spark_alpha(spark.kind, k));
            m.emissive = LinearRgba::rgb(r * e, g * e, b * e);
        }
    }
}

/// Turn every spark to face the camera about the VERTICAL axis only, so the
/// upward stretch of a lick or ember stays upright from any angle. The rig is
/// unrotated, so a child's local yaw is its world yaw.
pub fn billboard_immolate_sparks(
    camera: Query<&GlobalTransform, With<Camera3d>>,
    rigs: Query<(&Transform, &Children), (With<ImmolateBurnRig>, Without<ImmolateSpark>)>,
    mut sparks: Query<&mut Transform, (With<ImmolateSpark>, Without<ImmolateBurnRig>)>,
) {
    let Some(cam) = camera.iter().next() else {
        return;
    };
    let cam = cam.translation();
    for (rig, children) in rigs.iter() {
        for child in children.iter() {
            if let Ok(mut part) = sparks.get_mut(child) {
                let to_cam = cam - (rig.translation + part.translation);
                // A `Rectangle` faces +Z; yaw +Z onto the camera direction.
                part.rotation = Quat::from_rotation_y(to_cam.x.atan2(to_cam.z));
            }
        }
    }
}

/// End the fire the frame the Immolate DoT is gone (expiry and dispel are one
/// path — the detector keys on presence) or the victim dies or despawns.
/// Despawning the rig takes its sparks with it; the fire does not linger.
pub fn cleanup_immolate_burns(
    mut commands: Commands,
    rigs: Query<(Entity, &ImmolateBurnRig)>,
    targets: Query<(&Combatant, Option<&ActiveAuras>)>,
) {
    for (entity, rig) in rigs.iter() {
        let lives = targets
            .get(rig.target)
            .map(|(c, a)| c.is_alive() && has_dot_state(a, DotStateVisual::ImmolateBurn))
            .unwrap_or(false);
        if !lives {
            commands.entity(entity).despawn();
        }
    }
}
