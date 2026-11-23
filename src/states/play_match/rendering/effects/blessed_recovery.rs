//! Blessed Recovery — Renew's amber rings swelling from the Priest's chest as
//! the proc fires (AS-352).
//!
//! From the client data joined in
//! `docs/design/2026-10-17-priest-talent-client-data.md` §10 (AS-335; the
//! procs 27813 / 27817 / 27818, SpellVisual 7548): the impact is kit 435,
//! `renew_chest.m2` at the chest — Renew's and Greater Heal's — and there is
//! no `(7,8)` state, so the 6 s heal over time draws nothing after it lands,
//! and its ticks draw nothing.
//!
//! Played as the signed-off AS-335 Priest Talent Visuals Bench draws it
//! (https://claude.ai/artifact/PaMJeCzqUTVThWXZcNsyoc, version
//! 1792207490-53c9, the "Blessed Recovery" tab; its source as committed with
//! the sign-off is `docs/design/benches/2026-10-17-priest-talent-bench.html`):
//! both layers on, every knob at the client's value. The tab's two emitter
//! records, one for one, from the chest (`ANCH.chest`, 1.45 yd):
//!
//!   rings -> 9.2/s for 700 ms, out in every direction at 0.18 yd/s under
//!            drag 0.3, living 0.76 s, radius 0.28 -> 0.49 -> 1.09 yd, amber
//!            (255,157,3) -> (250,219,8) -> (253,179,20), alpha 0.15 -> 0.57
//!            -> 0;
//!   glows -> the same emission, radius 0.22 -> 0.7 -> 0.56 yd, orange
//!            (255,150,40) -> white (255,250,240), alpha 0.3 -> 0.45 -> 0.
//!
//! Both draw through the bench's `Emitter.draw` with no `blend: 'alpha'`, so
//! both are `lighter`: `AlphaMode::Add`. Both are the bench's `sprite('glow')`
//! (the client's `shockwavewater1` rings are drawn as the bench draws them,
//! soft glows, as Aura Mastery's ripples are). Six of each are born: the
//! bench's accumulator first owes a whole particle at 1/9.2 s, and the window
//! shuts at 700 ms, past the sixth (652 ms) and short of the seventh (761
//! ms). The rig lives until the sixth has lived out its life, 1.412 s.
//!
//! **Lifecycle.** Rides the sim's `AuraLanding` marker: `apply_pending_auras`
//! spawns one each time the proc's heal over time lands, fresh or refreshed,
//! so each proc plays it once, at the proc. It plays out on a Priest that
//! dies under it (a landing, as Vampiric Embrace's implosion), goes at once
//! with one that is gone, and is hidden while its Priest is a sheep (the
//! sheep wears nothing), its clock running on. A `PlayMatchEntity`.
//!
//! **Departures from the bench**, each deliberate:
//!
//! 1. **Held in front of the body.** Every particle is drawn on its own view
//!    ray, slid in front of the Priest's RENDERED body and scaled by the same
//!    ratio (the shared body hold, `body_hold.rs`), so the half of the burst
//!    deeper than the body's centre is not painted over by the body.
//! 2. **The drag is integrated exactly.** The bench slows a particle by
//!    `1 − 3 × drag × dt` a frame; here its speed decays as
//!    `e^(−3 × drag × t)`, the same curve at any frame rate.
//! 3. **Particles ride the rig in height too.** The bench's `follow` moves
//!    them with the body in x and z only.
//!
//! **At the match camera** (60 yd, pitch 38.7°, FOV 45°, about 22 px a yard at
//! 1080p): the rings grow from about 12 px to 48 px across at under 0.6
//! alpha; the glows reach about 31 px.
//!
//! Graphical-only: nothing here writes sim state, no `game_rng` is drawn (the
//! scatter is the bench's own hash), and every system is registered in
//! `states/mod.rs` only. Headless never runs any of it.

use std::f32::consts::TAU;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;

use super::body_hold::{held_in, BodyHold, HoldDisjoint};
use super::cheat_death::{particle_sprite_image, GLOW_SOFT};
use super::druid_heals::{body_point, keyed};
use super::paladin_oneshots::bench_hash;
use crate::states::play_match::components::*;

// ── The bench's constants block (signed off at version 1792207490-53c9) ─────

// Blessed Recovery proc: renew_chest.m2 (kit 435) one-shot on the Priest. AS-335 bench.
pub const BLESSED_RECOVERY_RINGS: bool = true;
pub const BLESSED_RECOVERY_GLOWS: bool = true;
// HoT ticks: no visual.

// ── renew_chest.m2 ───────────────────────────────────────────────────────────

/// The chest attachment, yards above the feet (the bench's `ANCH.chest`).
pub const RECOVERY_CHEST: f32 = 1.45;
/// Both layers: particles per second while the window is open, the window,
/// their speed, life and drag (the bench's `drag`; the speed decays at
/// `3 × drag` a second).
pub const RECOVERY_RATE: f32 = 9.2;
pub const RECOVERY_WINDOW_SECS: f32 = 0.7;
pub const RECOVERY_SPEED: f32 = 0.18;
pub const RECOVERY_LIFE: f32 = 0.76;
pub const RECOVERY_DRAG: f32 = 0.3;
/// Particles each layer bears: the bench's accumulator owes the `k`-th at
/// `k / rate`, and the window shuts before the seventh.
pub const RECOVERY_BIRTHS: u8 = 6;

/// One of the two layers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum RecoveryLayer {
    /// `shockwavewater1`, the amber rings.
    Rings,
    /// `yellow_glow3a`, the orange-white glows.
    Glows,
}

impl RecoveryLayer {
    pub const ALL: [RecoveryLayer; 2] = [RecoveryLayer::Rings, RecoveryLayer::Glows];

    /// The bench's layer toggle.
    pub fn on(self) -> bool {
        match self {
            RecoveryLayer::Rings => BLESSED_RECOVERY_RINGS,
            RecoveryLayer::Glows => BLESSED_RECOVERY_GLOWS,
        }
    }

    /// Its colour over a particle's life.
    pub fn rgb(self) -> &'static [(f32, [u8; 3])] {
        match self {
            RecoveryLayer::Rings => &[
                (0.0, [255, 157, 3]),
                (0.5, [250, 219, 8]),
                (1.0, [253, 179, 20]),
            ],
            RecoveryLayer::Glows => &[(0.0, [255, 150, 40]), (1.0, [255, 250, 240])],
        }
    }

    /// Its alpha over a particle's life.
    pub fn alpha(self) -> &'static [(f32, f32)] {
        match self {
            RecoveryLayer::Rings => &[(0.0, 0.15), (0.5, 0.57), (1.0, 0.0)],
            RecoveryLayer::Glows => &[(0.0, 0.3), (0.5, 0.45), (1.0, 0.0)],
        }
    }

    /// The bench's `scale`: the sprite's RADIUS, yards, over its life.
    pub fn radius(self) -> &'static [(f32, f32)] {
        match self {
            RecoveryLayer::Rings => &[(0.0, 0.28), (0.5, 0.49), (1.0, 1.09)],
            RecoveryLayer::Glows => &[(0.0, 0.22), (0.5, 0.7), (1.0, 0.56)],
        }
    }

    fn index(self) -> u32 {
        self as u32
    }
}

/// When particle `j` (0-based) of either layer is born, seconds after the
/// proc: `(j + 1) / rate`, for the [`RECOVERY_BIRTHS`] the window holds.
pub fn recovery_birth(j: u8) -> Option<f32> {
    let at = (j as f32 + 1.0) / RECOVERY_RATE;
    (j < RECOVERY_BIRTHS && at < RECOVERY_WINDOW_SECS).then_some(at)
}

/// The rig's whole life: until the last particle born has lived out its life.
pub fn recovery_secs() -> f32 {
    recovery_birth(RECOVERY_BIRTHS - 1).unwrap_or(0.0) + RECOVERY_LIFE
}

/// How far a particle launched at `speed` has flown `age` seconds on, under
/// the drag: `speed (1 − e^(−k t)) / k`, `k = 3 × drag`.
pub fn recovery_flight(speed: f32, age: f32) -> f32 {
    let k = 3.0 * RECOVERY_DRAG;
    speed * (1.0 - (-k * age).exp()) / k
}

/// One particle's look at a moment.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RecoveryPose {
    /// Where it truly is, from the chest.
    pub at: Vec3,
    /// The sprite's full width, yards.
    pub size: f32,
    pub rgb: [u8; 3],
    pub alpha: f32,
    /// Its roll in the camera's plane (the bench's `ang`).
    pub roll: f32,
}

fn scatter(layer: RecoveryLayer, j: u32, k: u32) -> f32 {
    bench_hash(
        j.wrapping_mul(3)
            .wrapping_add(layer.index())
            .wrapping_mul(8)
            .wrapping_add(k)
            .wrapping_add(4352),
    )
}

/// A colour track at `t`, interpolated in sRGB bytes and truncated, as the
/// bench's canvas does (`rgb()`'s `|0`).
fn track_rgb(track: &[(f32, [u8; 3])], t: f32) -> [u8; 3] {
    let channel = |c: usize| {
        let keys: Vec<(f32, f32)> = track.iter().map(|(t, v)| (*t, v[c] as f32)).collect();
        keyed(&keys, t).floor().clamp(0.0, 255.0) as u8
    };
    [channel(0), channel(1), channel(2)]
}

/// Particle `j`'s direction: the bench's `vRange` 3.14, a random point of
/// the cube `[-1, 1]³`, normalised.
pub fn recovery_direction(layer: RecoveryLayer, j: u8) -> Vec3 {
    let j = j as u32;
    Vec3::new(
        scatter(layer, j, 1) * 2.0 - 1.0,
        scatter(layer, j, 2) * 2.0 - 1.0,
        scatter(layer, j, 3) * 2.0 - 1.0,
    )
    .try_normalize()
    .unwrap_or(Vec3::Y)
}

/// Particle `j` of `layer`, `age` seconds after the proc; `None` before its
/// birth and after its life. Pure: the probes and the placement read one
/// definition.
pub fn recovery_pose(layer: RecoveryLayer, j: u8, age: f32) -> Option<RecoveryPose> {
    let page = age - recovery_birth(j)?;
    if !(0.0..RECOVERY_LIFE).contains(&page) {
        return None;
    }
    let t = page / RECOVERY_LIFE;
    Some(RecoveryPose {
        at: recovery_direction(layer, j) * recovery_flight(RECOVERY_SPEED, page),
        size: 2.0 * keyed(layer.radius(), t),
        rgb: track_rgb(layer.rgb(), t),
        alpha: keyed(layer.alpha(), t),
        // The canvas turns a sprite clockwise on screen.
        roll: -scatter(layer, j as u32, 7) * TAU,
    })
}

/// One drawn particle of the burst.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub struct RecoveryPiece {
    pub layer: RecoveryLayer,
    /// Its birth index, 0..[`RECOVERY_BIRTHS`].
    pub j: u8,
}

/// The burst's particles, by its layer toggles.
pub fn recovery_pieces() -> Vec<RecoveryPiece> {
    RecoveryLayer::ALL
        .into_iter()
        .filter(|l| l.on())
        .flat_map(|layer| (0..RECOVERY_BIRTHS).map(move |j| RecoveryPiece { layer, j }))
        .collect()
}

// ── Components ───────────────────────────────────────────────────────────────

/// One proc's burst at a Priest's chest, parent of its particles.
#[derive(Component, Debug)]
pub struct BlessedRecoveryBurst {
    pub priest: Entity,
    pub age: f32,
}

/// One particle of a [`BlessedRecoveryBurst`].
#[derive(Component, Debug)]
pub struct BlessedRecoveryParticle {
    pub piece: RecoveryPiece,
}

// ── Systems ──────────────────────────────────────────────────────────────────

fn hidden(sheep: bool) -> Visibility {
    if sheep {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    }
}

/// Update (graphical-only): a burst for every Blessed Recovery proc the sim
/// landed. Only this landing's markers are consumed; a Priest that is gone
/// gets nothing.
#[allow(clippy::type_complexity)]
pub fn spawn_blessed_recovery_bursts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut assets: Local<Option<(Handle<Mesh>, Handle<Image>)>>,
    landings: Query<(Entity, &AuraLanding)>,
    priests: Query<Has<PolymorphedVisual>, With<Combatant>>,
) {
    for (marker, landing) in landings.iter() {
        if landing.kind != AuraLandingKind::BlessedRecoveryRings {
            continue;
        }
        commands.entity(marker).despawn();
        let Ok(sheep) = priests.get(landing.target) else {
            continue;
        };
        let (quad, sprite) = assets
            .get_or_insert_with(|| {
                (
                    meshes.add(Rectangle::new(1.0, 1.0)),
                    images.add(particle_sprite_image(GLOW_SOFT)),
                )
            })
            .clone();
        let rig = commands
            .spawn((
                BlessedRecoveryBurst {
                    priest: landing.target,
                    age: 0.0,
                },
                Transform::default(),
                hidden(sheep),
                PlayMatchEntity,
            ))
            .id();
        for piece in recovery_pieces() {
            let material = materials.add(StandardMaterial {
                base_color: Color::WHITE.with_alpha(0.0),
                base_color_texture: Some(sprite.clone()),
                // Unlit: the bench adds colour × sprite × alpha and nothing
                // else.
                unlit: true,
                alpha_mode: AlphaMode::Add,
                cull_mode: None,
                double_sided: true,
                ..default()
            });
            let child = commands
                .spawn((
                    BlessedRecoveryParticle { piece },
                    Mesh3d(quad.clone()),
                    MeshMaterial3d(material),
                    Transform::from_scale(Vec3::splat(1e-3)),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(rig).add_child(child);
        }
    }
}

/// Update (graphical-only): age each burst, hide it while its Priest is a
/// sheep, and retire it after its life or at once when its Priest is gone.
pub fn update_blessed_recovery_bursts(
    mut commands: Commands,
    time: Res<Time>,
    mut rigs: Query<(Entity, &mut BlessedRecoveryBurst, &mut Visibility)>,
    priests: Query<Has<PolymorphedVisual>, With<Combatant>>,
) {
    let dt = time.delta_secs();
    for (entity, mut burst, mut visibility) in rigs.iter_mut() {
        let Ok(sheep) = priests.get(burst.priest) else {
            commands.entity(entity).despawn();
            continue;
        };
        burst.age += dt;
        if burst.age >= recovery_secs() {
            commands.entity(entity).despawn();
            continue;
        }
        visibility.set_if_neq(hidden(sheep));
    }
}

/// PostUpdate (graphical-only), after every writer of a body's transform and
/// before propagation: stand each burst at its Priest's RENDERED chest, and
/// place each particle where it is drawn — held in front of that body for the
/// camera — turned to the camera, sized and coloured along its life. The rig
/// is unrotated, so a particle's local pose is its world offset.
#[allow(clippy::type_complexity)]
pub fn place_blessed_recovery_bursts(
    hold: BodyHold,
    mut rigs: Query<
        (&BlessedRecoveryBurst, &mut Transform, &Children),
        (Without<BlessedRecoveryParticle>, HoldDisjoint),
    >,
    mut particles: Query<
        (
            &BlessedRecoveryParticle,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Without<BlessedRecoveryBurst>, HoldDisjoint),
    >,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let view = hold.view();
    for (burst, mut rig_transform, children) in rigs.iter_mut() {
        let Some(body) = hold.body(burst.priest) else {
            continue;
        };
        let chest = body_point(RECOVERY_CHEST, body.root, false);
        rig_transform.translation = chest;
        let capsule = body.capsule();
        for child in children.iter() {
            let Ok((particle, mut transform, material)) = particles.get_mut(child) else {
                continue;
            };
            let pose = recovery_pose(particle.piece.layer, particle.piece.j, burst.age);
            let (rgb, alpha) = match pose {
                Some(p) => {
                    let at = chest + p.at;
                    let (drawn, k) = held_in(view, at, capsule);
                    transform.translation = drawn - chest;
                    transform.rotation = view.map(|c| c.rotation).unwrap_or_default()
                        * Quat::from_rotation_z(p.roll);
                    transform.scale = Vec3::new(p.size * k, p.size * k, 1.0).max(Vec3::splat(1e-3));
                    (p.rgb, p.alpha.clamp(0.0, 1.0))
                }
                None => {
                    transform.scale = Vec3::splat(1e-3);
                    ([255, 255, 255], 0.0)
                }
            };
            let color = Color::srgb_u8(rgb[0], rgb[1], rgb[2]).with_alpha(alpha);
            // Only touch the asset when the look moved: `get_mut` marks it
            // changed and re-uploads it.
            if materials
                .get(&material.0)
                .is_some_and(|m| m.base_color != color)
            {
                if let Some(m) = materials.get_mut(&material.0) {
                    m.base_color = color;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Six of each layer, the last born at 652 ms, the rig gone at 1.412 s.
    #[test]
    fn six_of_each_are_born_inside_the_window() {
        assert_eq!(recovery_birth(0), Some(1.0 / 9.2));
        assert!((recovery_birth(5).unwrap() - 6.0 / 9.2).abs() < 1e-6);
        assert_eq!(recovery_birth(6), None);
        const { assert!(7.0 / 9.2 > RECOVERY_WINDOW_SECS) };
        assert!((recovery_secs() - (6.0 / 9.2 + 0.76)).abs() < 1e-6);
        assert_eq!(recovery_pieces().len(), 12);
    }

    /// The flight under drag tends to `speed / 3 drag` and starts at `speed`.
    #[test]
    fn the_drag_is_the_benchs_decay() {
        let k = 0.9_f32;
        assert!(recovery_flight(0.18, 0.0).abs() < 1e-7);
        let d = 1e-3;
        assert!(((recovery_flight(0.18, d) / d) - 0.18).abs() < 1e-3);
        assert!(
            (recovery_flight(0.18, 0.5) - 0.18 * (1.0 - (-k * 0.5_f32).exp()) / k).abs() < 1e-7
        );
    }
}
