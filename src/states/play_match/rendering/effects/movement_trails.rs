use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Charge Trail Visual (Warrior Charge + Boar Charge)
// ==============================================================================
//
// Grounded in the Classic client data (docs/design/2026-09-06-charge-client-data.md):
// Charge ranks 100/6178/11578 all resolve to SpellVisual 867, whose single
// caster kit (44) attaches two models for the duration of the dash:
//   - `spells/chargetrail.m2` at the chest — ONE RIBBON: red (0.81, 0, 0),
//     alpha ~0.40, half-height 0.47 units, 1.0s edge lifetime, zero gravity.
//     A translucent red streamer painted along the actual dash path.
//   - `spells/dustcloud_land.m2` at the base — alpha-blended smoke puffs
//     (lives 0.9–1.0s) kicked up at the feet.
// So the source is emphatically NOT a solid volume at the dash origin: it is a
// sequence of path-following elements that fade behind the runner. This module
// renders that as distance-paced emission along the mover's live transform —
// thin vertical streak segments at chest height (the ribbon analog) plus
// ground-level dust puffs — shared by the Warrior and the Boar (same
// `ChargingState` gap-closer), scaled down for the Boar's body.

/// Distance between emitted trail elements along the path (yards, at scale 1).
const EMIT_SPACING: f32 = 0.55;
/// Fraction of the emission spacing a streak segment actually fills. Kept
/// well under 1.0 so consecutive segments leave visible gaps — the trail must
/// read as a sequence of discrete dissolving streaks, not fuse back into the
/// solid slab this card replaced.
const STREAK_FILL: f32 = 0.55;
/// Nominal streak segment fade time. Short relative to the ~28 yd/s dash so
/// the tail visibly dissolves behind the runner (a comet tail, not a wall);
/// jittered per segment (see `hash01`) so no two neighbors fade in lockstep.
const STREAK_LIFETIME: f32 = 0.32;
/// Per-segment lifetime jitter half-range (fraction of `STREAK_LIFETIME`).
const STREAK_LIFE_JITTER: f32 = 0.15;
/// Dust particle life range (staggered per particle; client puffs live
/// 0.9–1.0s, but small fast puffs read better at roughly half that).
const DUST_LIFE_MIN: f32 = 0.4;
const DUST_LIFE_MAX: f32 = 0.7;
/// Ribbon half-height (client: heightAbove = heightBelow = 0.472 units).
const STREAK_HALF_HEIGHT: f32 = 0.47;
/// Chest offset above the charger's BODY CENTRE at scale 1 — the repo's shared
/// chest convention (`school_impact::IMPACT_CHEST_Y`); the client ribbon
/// straddles its chest attachment symmetrically (heightAbove = heightBelow),
/// so the segment centres exactly there. Body centre is `translation.y +
/// rest_y`, NOT the sim y — see `ChargeTrailEmitter::rest_y`.
const STREAK_CHEST_OFFSET: f32 = super::IMPACT_CHEST_Y;
/// Dust particle base radius range at scale 1 (small puffs, not pearls).
const DUST_RADIUS_MIN: f32 = 0.05;
const DUST_RADIUS_MAX: f32 = 0.12;
/// Horizontal scatter range of a cluster's particles around the emission
/// point (yards, at scale 1).
const DUST_SCATTER_MIN: f32 = 0.15;
const DUST_SCATTER_MAX: f32 = 0.3;
/// Body-size scale for pet chargers (the Boar).
const PET_SCALE: f32 = 0.55;

/// Walk the segment between the last emitted point and the mover's current
/// position, invoking `emit(mid, dir)` every `spacing` yards of actual
/// travel and advancing `last_emit`. The loop matters: one fast frame can
/// cover several spacings, and each element must land ON the path, not at
/// the endpoint. Shared by every path-laid trail (Charge, Disengage).
fn emit_along_path(last_emit: &mut Vec3, pos: Vec3, spacing: f32, mut emit: impl FnMut(Vec3, Vec3)) {
    loop {
        let delta = pos - *last_emit;
        let dist = delta.length();
        if dist < spacing {
            break;
        }
        let dir = delta / dist;
        let next = *last_emit + dir * spacing;
        let mid = (*last_emit + next) * 0.5;
        emit(mid, dir);
        *last_emit = next;
    }
}

/// Deterministic 0..1 hash — per-element variation without touching
/// `game_rng` (this is a render-only system; drawing the sim RNG here would
/// break headless/graphical seed parity).
fn hash01(seed: u32) -> f32 {
    let mut h = seed.wrapping_mul(0x9E37_79B9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h & 0x00FF_FFFF) as f32 / 16_777_216.0
}

/// Seed a hash stream from a world position (plus a salt for independent
/// draws) — two clusters at different points along the path never match.
fn seed_from(pos: Vec3, salt: u32) -> u32 {
    pos.x.to_bits() ^ pos.z.to_bits().rotate_left(13) ^ salt.wrapping_mul(0x0068_5DA5)
}

fn spawn_streak_segment(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    mid: Vec3,
    dir: Vec3,
    length: f32,
    scale: f32,
    rest_y: f32,
) {
    // Deliberately shorter than the emission spacing: the gaps are what make
    // the trail read as a sequence of discrete streaks instead of one slab.
    let mesh = meshes.add(Cuboid::new(
        length * STREAK_FILL,
        STREAK_HALF_HEIGHT * 2.0 * scale,
        0.05,
    ));
    // Client ribbon color: red (0.81, 0, 0) at alpha ~0.40.
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.81, 0.06, 0.04, 0.4),
        emissive: LinearRgba::new(1.4, 0.12, 0.08, 1.0),
        alpha_mode: AlphaMode::Add,
        ..default()
    });

    // Pure yaw: local +X onto the horizontal travel direction, so the
    // segment's height axis stays world-vertical whatever the heading.
    let yaw = (-dir.z).atan2(dir.x);
    // Anchor off the RENDERED body, not the sim entity: `mid.y + rest_y` is
    // the body centre for both unit kinds (a pet sims ~1.45yd above its
    // capsule; a combatant's rest_y is 0) — the `hard_cc.rs` stun-whirl
    // derivation.
    let pos = Vec3::new(
        mid.x,
        mid.y + rest_y + STREAK_CHEST_OFFSET * scale,
        mid.z,
    );

    // Jittered life so adjacent segments never fade in lockstep — even two
    // segments emitted in the same fast frame dissolve on their own clocks.
    let life = STREAK_LIFETIME
        * (1.0 + STREAK_LIFE_JITTER * (2.0 * hash01(seed_from(mid, 0)) - 1.0));

    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
        ChargeStreakSegment {
            lifetime: life,
            initial_lifetime: life,
        },
        PlayMatchEntity,
    ));
}

/// Kick up a small CLUSTER of dust at one emission point: 3–5 varied-size
/// puffs scattered around it, drifting up and outward as they expand and
/// fade. One big uniform sphere per point read as pearls on a string; the
/// cluster's per-particle variation (all hashed off the position — no
/// `game_rng`) is what makes it read as kicked-up smoke.
fn spawn_dust_cluster(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    at: Vec3,
    scale: f32,
) {
    let count = 3 + (hash01(seed_from(at, 100)) * 3.0) as u32; // 3..=5
    for i in 0..count {
        let salt = 101 + i * 7;
        let h = |k: u32| hash01(seed_from(at, salt + k * 31));

        let radius = (DUST_RADIUS_MIN + (DUST_RADIUS_MAX - DUST_RADIUS_MIN) * h(0)) * scale;
        let angle = std::f32::consts::TAU * h(1);
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        let scatter = (DUST_SCATTER_MIN + (DUST_SCATTER_MAX - DUST_SCATTER_MIN) * h(2)) * scale;
        // Slight upward + outward drift; the upward component keeps every
        // particle above the floor for its whole life (the round-3 bound).
        let velocity = (out * (0.3 + 0.3 * h(3)) + Vec3::Y * (0.3 + 0.4 * h(4))) * scale;
        let life = DUST_LIFE_MIN + (DUST_LIFE_MAX - DUST_LIFE_MIN) * h(5);
        let spawn_y = (0.05 + 0.10 * h(6)) * scale;

        let mesh = meshes.add(Sphere::new(radius));
        // Dusty tan, kept dim: the dust is grounding, not glow (the client's
        // puffs are alpha-blended smoke; Add is the repo's Z-fighting-safe
        // idiom, so the smoke read comes from low emissive instead of true
        // alpha).
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.55, 0.48, 0.38, 0.35),
            emissive: LinearRgba::new(0.35, 0.30, 0.22, 1.0),
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        let pos = Vec3::new(at.x, spawn_y, at.z) + out * scatter;

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(pos),
            ChargeDustPuff {
                lifetime: life,
                initial_lifetime: life,
                base_scale: 1.0,
                velocity,
            },
            PlayMatchEntity,
        ));
    }
}

/// Emit the charge trail along the dash path.
///
/// Runs every frame while anything carries `ChargingState` — the Warrior's
/// Charge and the Boar's are the same gap-closer dash (`move_to_target`
/// advances both identically), so they share one trail, scaled to body size.
/// Emission is distance-paced: elements are laid every `EMIT_SPACING` yards
/// of actual travel, walking the segment between the last emitted point and
/// the mover's current transform, so the trail hugs the real path at any
/// frame rate.
pub fn spawn_charge_trail(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut chargers: Query<
        (
            Entity,
            &Transform,
            Option<&mut ChargeTrailEmitter>,
            Option<&Pet>,
            Option<&Children>,
        ),
        With<ChargingState>,
    >,
    bodies: Query<&VisualBody>,
    stale_emitters: Query<Entity, (With<ChargeTrailEmitter>, Without<ChargingState>)>,
) {
    for (entity, transform, emitter, pet, children) in chargers.iter_mut() {
        let pos = transform.translation;
        match emitter {
            None => {
                // Dash just started: arm the emitter and kick up launch dust
                // (the client's `dustcloud_land` burst at the base).
                let scale = if pet.is_some() { PET_SCALE } else { 1.0 };
                // The streak anchors off the RENDERED body — see
                // `ChargeTrailEmitter::rest_y`. Absent a body child, 0
                // degrades to the sim y, which is correct for a combatant.
                let rest_y = children
                    .and_then(|cs| cs.iter().find_map(|c| bodies.get(c).ok()))
                    .map_or(0.0, |b| b.rest_y);
                commands
                    .entity(entity)
                    .try_insert(ChargeTrailEmitter { last_emit: pos, scale, rest_y });
                spawn_dust_cluster(&mut commands, &mut meshes, &mut materials, pos, scale * 1.4);
            }
            Some(mut em) => {
                let spacing = EMIT_SPACING * em.scale;
                let (scale, rest_y) = (em.scale, em.rest_y);
                emit_along_path(&mut em.last_emit, pos, spacing, |mid, dir| {
                    spawn_streak_segment(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        mid,
                        dir,
                        spacing,
                        scale,
                        rest_y,
                    );
                    spawn_dust_cluster(&mut commands, &mut meshes, &mut materials, mid, scale);
                });
            }
        }
    }

    // The dash ended (ChargingState removed): disarm, so a later charge by
    // the same entity starts a fresh trail instead of drawing a streak from
    // the old end point across the map.
    for entity in stale_emitters.iter() {
        commands.entity(entity).remove::<ChargeTrailEmitter>();
    }
}

// ==============================================================================
// Disengage Trail Visual (Hunter backward leap)
// ==============================================================================
//
// AUTHORED design — the Classic Era client has no leap visual to port. In
// 1.15.9 Disengage (781/14272/14273 → SpellVisual 738) is the vanilla melee
// threat-drop: HasMissile = 0, no positioners, no area model, no ribbon —
// just a Special1H swipe anim and a one-shot chest flash on caster and
// target (`spells/blink_impact_chest.m2`: nine Add-blended flare / star /
// sparkle / pixie emitters, white-blue, lives 0.35–1.14 s). The backward
// leap is a Wrath-era mechanic this sim adopted, so its trail borrows the
// Charge trail's client-grounded path-laid construction — distance-paced
// elements laid along the live transform, per-element fade — while the
// palette and particle vocabulary come from Disengage's own kit model: thin
// wind slivers (speed-lines, not Charge's tall red ribbon) plus tiny
// white-blue spark motes, and a launch flash at the jump point (the
// blink-flash analog). No ground elements: this is an air move.

/// Distance between emitted trail elements along the leap path (yards).
const WIND_EMIT_SPACING: f32 = 0.55;
/// Fraction of the emission spacing a wind sliver actually fills — gaps keep
/// the trail reading as streaking air, not a solid pipe.
const WIND_STREAK_FILL: f32 = 0.6;
/// Wind sliver cross-section (thin speed-lines, nothing like the ~1-yd-tall
/// Charge ribbon band).
const WIND_STREAK_HEIGHT: f32 = 0.08;
const WIND_STREAK_DEPTH: f32 = 0.04;
/// Vertical jitter half-range of sliver centres around the chest anchor.
const WIND_HEIGHT_JITTER: f32 = 0.4;
/// Lateral jitter half-range of slivers off the path line (yards).
const WIND_LATERAL_JITTER: f32 = 0.25;
/// Nominal sliver fade time; the leap lasts 0.5 s (15 yd at 30 yd/s), so the
/// tail dissolves visibly behind the Hunter mid-flight.
const WIND_LIFETIME: f32 = 0.3;
/// Per-sliver lifetime jitter half-range (fraction of `WIND_LIFETIME`).
const WIND_LIFE_JITTER: f32 = 0.15;
/// Spark mote radius range (tiny additive glints, client flare/pixie analog).
const MOTE_RADIUS_MIN: f32 = 0.03;
const MOTE_RADIUS_MAX: f32 = 0.07;
/// Spark mote life range (client sparkle lives 0.35–1.14 s; short end fits
/// the half-second leap).
const MOTE_LIFE_MIN: f32 = 0.3;
const MOTE_LIFE_MAX: f32 = 0.55;
/// Spark mote scatter radius range around an emission point (yards).
const MOTE_SCATTER_MIN: f32 = 0.15;
const MOTE_SCATTER_MAX: f32 = 0.4;
/// Chest anchor above the sim transform — only combatants Disengage, so
/// unlike the Charge emitter there is no pet `rest_y` correction to carry.
const WIND_CHEST_OFFSET: f32 = super::IMPACT_CHEST_Y;

fn spawn_wind_slivers(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    mid: Vec3,
    dir: Vec3,
    length: f32,
) {
    // The horizontal travel heading and its lateral normal (the leap
    // direction is horizontal; `dir` comes from the actual traveled delta).
    let yaw = (-dir.z).atan2(dir.x);
    let lateral = Vec3::new(-dir.z, 0.0, dir.x).normalize_or_zero();

    let count = 2 + (hash01(seed_from(mid, 200)) * 2.0) as u32; // 2..=3
    for i in 0..count {
        let salt = 201 + i * 13;
        let h = |k: u32| hash01(seed_from(mid, salt + k * 29));

        let mesh = meshes.add(Cuboid::new(
            length * WIND_STREAK_FILL,
            WIND_STREAK_HEIGHT,
            WIND_STREAK_DEPTH,
        ));
        // Pale blue-white air, faint glow — the wind palette (and the client
        // kit's white-blue), not Charge's red.
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.85, 0.92, 1.0, 0.35),
            emissive: LinearRgba::new(1.1, 1.4, 1.9, 1.0),
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        let pos = Vec3::new(mid.x, mid.y + WIND_CHEST_OFFSET, mid.z)
            + Vec3::Y * (WIND_HEIGHT_JITTER * (2.0 * h(0) - 1.0))
            + lateral * (WIND_LATERAL_JITTER * (2.0 * h(1) - 1.0));

        let life = WIND_LIFETIME * (1.0 + WIND_LIFE_JITTER * (2.0 * h(2) - 1.0));

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(pos).with_rotation(Quat::from_rotation_y(yaw)),
            DisengageWindStreak {
                lifetime: life,
                initial_lifetime: life,
            },
            PlayMatchEntity,
        ));
    }
}

/// Scatter a handful of spark motes around one point at body height: tiny
/// additive white-blue glints (the flare / sparkle / pixie emitters of
/// `blink_impact_chest.m2`) drifting gently as they fade. `burst` widens the
/// scatter and count and throws the motes outward — the one-shot launch
/// flash at the jump point.
fn spawn_spark_motes(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    at: Vec3,
    burst: bool,
) {
    let count = if burst {
        5 + (hash01(seed_from(at, 300)) * 3.0) as u32 // 5..=7
    } else {
        2 + (hash01(seed_from(at, 300)) * 2.0) as u32 // 2..=3
    };
    for i in 0..count {
        let salt = 301 + i * 11;
        let h = |k: u32| hash01(seed_from(at, salt + k * 37));

        let radius = MOTE_RADIUS_MIN + (MOTE_RADIUS_MAX - MOTE_RADIUS_MIN) * h(0);
        let angle = std::f32::consts::TAU * h(1);
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        let scatter_max = if burst { MOTE_SCATTER_MAX * 1.5 } else { MOTE_SCATTER_MAX };
        let scatter = MOTE_SCATTER_MIN + (scatter_max - MOTE_SCATTER_MIN) * h(2);
        // Trail motes hang in the air with a gentle rise; the launch burst
        // throws them outward (the client flash's radial sparkle shell).
        let velocity = if burst {
            out * (0.8 + 0.7 * h(3)) + Vec3::Y * (0.2 + 0.3 * h(4))
        } else {
            out * (0.15 + 0.2 * h(3)) + Vec3::Y * (0.15 + 0.25 * h(4))
        };
        let life = MOTE_LIFE_MIN + (MOTE_LIFE_MAX - MOTE_LIFE_MIN) * h(5);
        let dy = WIND_CHEST_OFFSET + (0.5 * (2.0 * h(6) - 1.0));

        let mesh = meshes.add(Sphere::new(radius));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.9, 0.95, 1.0, 0.5),
            emissive: LinearRgba::new(1.5, 1.8, 2.4, 1.0),
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        let pos = Vec3::new(at.x, at.y + dy, at.z) + out * scatter;

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(pos),
            DisengageSparkMote {
                lifetime: life,
                initial_lifetime: life,
                velocity,
            },
            PlayMatchEntity,
        ));
    }
}

/// Emit the Disengage wind trail along the leap path.
///
/// Runs every frame while anything carries `DisengagingState`; emission is
/// distance-paced along the live transform exactly like the Charge trail, so
/// the trail hugs the real leap path (obstacle slides included) at any frame
/// rate — the replacement for the old single wind-streak cylinder parked at
/// the leap origin.
pub fn spawn_disengage_trail(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut leapers: Query<
        (Entity, &Transform, Option<&mut DisengageTrailEmitter>),
        With<DisengagingState>,
    >,
    stale_emitters: Query<Entity, (With<DisengageTrailEmitter>, Without<DisengagingState>)>,
) {
    for (entity, transform, emitter) in leapers.iter_mut() {
        let pos = transform.translation;
        match emitter {
            None => {
                commands
                    .entity(entity)
                    .try_insert(DisengageTrailEmitter { last_emit: pos });
                spawn_spark_motes(&mut commands, &mut meshes, &mut materials, pos, true);
            }
            Some(mut em) => {
                emit_along_path(&mut em.last_emit, pos, WIND_EMIT_SPACING, |mid, dir| {
                    spawn_wind_slivers(
                        &mut commands,
                        &mut meshes,
                        &mut materials,
                        mid,
                        dir,
                        WIND_EMIT_SPACING,
                    );
                    spawn_spark_motes(&mut commands, &mut meshes, &mut materials, mid, false);
                });
            }
        }
    }

    // The leap ended (DisengagingState removed): disarm, so the next
    // Disengage starts a fresh trail instead of drawing a streak from the
    // old landing point across the map.
    for entity in stale_emitters.iter() {
        commands.entity(entity).remove::<DisengageTrailEmitter>();
    }
}

/// Update and cleanup Disengage trail elements: fade wind slivers, drift and
/// fade spark motes, despawn everything when expired.
pub fn update_and_cleanup_disengage_trails(
    mut commands: Commands,
    time: Res<Time>,
    mut slivers: Query<(Entity, &mut DisengageWindStreak, &MeshMaterial3d<StandardMaterial>)>,
    mut motes: Query<
        (
            Entity,
            &mut DisengageSparkMote,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        Without<DisengageWindStreak>,
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();

    for (entity, mut sliver, material_handle) in slivers.iter_mut() {
        sliver.lifetime -= dt;
        if sliver.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let progress = (sliver.lifetime / sliver.initial_lifetime).max(0.0);
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = Color::srgba(0.85, 0.92, 1.0, 0.35 * progress);
            material.emissive =
                LinearRgba::new(1.1 * progress, 1.4 * progress, 1.9 * progress, 1.0);
        }
    }

    for (entity, mut mote, mut transform, material_handle) in motes.iter_mut() {
        mote.lifetime -= dt;
        if mote.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let progress = (mote.lifetime / mote.initial_lifetime).max(0.0);
        transform.translation += mote.velocity * dt;
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = Color::srgba(0.9, 0.95, 1.0, 0.5 * progress);
            material.emissive =
                LinearRgba::new(1.5 * progress, 1.8 * progress, 2.4 * progress, 1.0);
        }
    }
}

/// Update and cleanup charge trail elements: fade streaks, grow-and-fade
/// dust, despawn everything when expired.
pub fn update_and_cleanup_charge_trails(
    mut commands: Commands,
    time: Res<Time>,
    mut streaks: Query<(Entity, &mut ChargeStreakSegment, &MeshMaterial3d<StandardMaterial>)>,
    mut puffs: Query<
        (
            Entity,
            &mut ChargeDustPuff,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        Without<ChargeStreakSegment>,
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();

    for (entity, mut streak, material_handle) in streaks.iter_mut() {
        streak.lifetime -= dt;
        if streak.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let progress = (streak.lifetime / streak.initial_lifetime).max(0.0);
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = Color::srgba(0.81, 0.06, 0.04, 0.4 * progress);
            material.emissive =
                LinearRgba::new(1.4 * progress, 0.12 * progress, 0.08 * progress, 1.0);
        }
    }

    for (entity, mut puff, mut transform, material_handle) in puffs.iter_mut() {
        puff.lifetime -= dt;
        if puff.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let progress = (puff.lifetime / puff.initial_lifetime).max(0.0);
        // Dust drifts up-and-outward while expanding as it dissipates (the
        // client puffs fly outward at ~4 u/s).
        transform.translation += puff.velocity * dt;
        let age = 1.0 - progress;
        transform.scale = Vec3::splat(puff.base_scale * (1.0 + 1.2 * age));
        if let Some(material) = materials.get_mut(&material_handle.0) {
            material.base_color = Color::srgba(0.55, 0.48, 0.38, 0.35 * progress);
            material.emissive =
                LinearRgba::new(0.35 * progress, 0.30 * progress, 0.22 * progress, 1.0);
        }
    }
}
