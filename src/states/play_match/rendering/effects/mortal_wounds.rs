//! Mortal Wounds heal fracture (graphical-only).
//!
//! The healing-reduction debuff gets no treatment on the victim's body. It
//! states itself at the only moment it costs anyone anything: when a heal lands
//! on an afflicted target, the share the debuff refused sheds off as cold ash
//! instead of sinking in.
//!
//! Why not a body treatment: a persistent tint would have to say "cannot be
//! mended" by convention, and it would put a third displacer on the shared
//! `OriginalBodyMaterial` restore slot that Fear and Polymorph already contend
//! for (see `shared-restore-slot-mutual-exclusion.md`). Keying on the heal
//! instead means no marker component, no body contention, and none of the
//! aura exit-path traps — the effect is transient and self-expiring.
//!
//! Keyed on the reduction, not the ability: Hunter's Aimed Shot applies an
//! identical `HealingReduction` debuff and gets the same tell for free.
//!
//! Registered only in `states/mod.rs`; headless spawns the `HealingRefused`
//! markers and never reads them.

use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// --- Tuning knobs -----------------------------------------------------------

/// Ash motes at a full (100%) refusal; scaled down by the actual fraction.
const ASH_MOTES_AT_FULL: f32 = 26.0;
/// Floor so even a small reduction shows a few motes rather than none.
const ASH_MOTES_MIN: u32 = 5;
/// Outward drift speed (yd/s).
const ASH_SHED_SPEED: f32 = 1.3;
/// Downward acceleration (yd/s²). Ash sinks; healing rises — the contrast is
/// what makes the shed read as refusal rather than as more healing.
const ASH_SINK: f32 = 1.4;
/// Mote lifetime (seconds).
const ASH_LIFETIME: f32 = 0.9;
/// Mote radius (yards).
const ASH_RADIUS: f32 = 0.055;
/// Spawn band around the target's torso (yards).
const ASH_SPAWN_RADIUS_MIN: f32 = 0.12;
const ASH_SPAWN_RADIUS_SPAN: f32 = 0.34;
const ASH_SPAWN_HEIGHT_MIN: f32 = 0.35;
const ASH_SPAWN_HEIGHT_SPAN: f32 = 1.7;
/// Drained, colourless ash — deliberately desaturated so it cannot be mistaken
/// for either the gold of a heal or the crimson of the strike.
const ASH_COLOR: (f32, f32, f32) = (0.62, 0.59, 0.54);

// --- Runtime component (graphical-only) -------------------------------------

/// One mote of refused healing: a transient, unattached world particle that
/// drifts outward, sinks, and self-expires. Not owner-scoped and not a child,
/// so nothing has to clean up after the target (mirrors `FearMote` / `DotDrip`).
#[derive(Component)]
pub struct RefusedHealMote {
    velocity: Vec3,
    lifetime: f32,
    initial_lifetime: f32,
}

/// Cheap deterministic jitter in `[0, 1)`. Visual-only — never `game_rng`.
fn ash_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    ((s >> 9) & 0xFFFF) as f32 / 65536.0
}

/// How many motes a given refusal sheds.
fn mote_count(refused_fraction: f32) -> u32 {
    let scaled = (ASH_MOTES_AT_FULL * refused_fraction.clamp(0.0, 1.0)).round() as u32;
    scaled.max(ASH_MOTES_MIN)
}

// --- Systems ----------------------------------------------------------------

/// Update (graphical-only): turn each `HealingRefused` marker into a burst of
/// ash, then despawn the marker.
///
/// Markers whose target has already despawned are still consumed — otherwise
/// they would accumulate for the rest of the match.
pub fn spawn_heal_fracture(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    signals: Query<(Entity, &HealingRefused)>,
    targets: Query<&Transform, With<Combatant>>,
    mut cached: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    for (signal_entity, signal) in signals.iter() {
        if let Ok(target_transform) = targets.get(signal.target) {
            let (r, g, b) = ASH_COLOR;
            let (mesh, material) = cached
                .get_or_insert_with(|| {
                    (
                        meshes.add(Sphere::new(ASH_RADIUS)),
                        materials.add(StandardMaterial {
                            base_color: Color::srgb(r, g, b),
                            // A whisper of emissive keeps ash readable in arena
                            // shadow. Opaque, not additive: additive can only
                            // ADD light, which would make "refused healing"
                            // glow like healing.
                            emissive: LinearRgba::new(0.10, 0.09, 0.08, 1.0),
                            unlit: true,
                            ..default()
                        }),
                    )
                })
                .clone();

            let origin = target_transform.translation;
            let count = mote_count(signal.refused_fraction);
            for i in 0..count {
                let seed = signal
                    .target
                    .index()
                    .wrapping_mul(2_246_822_519)
                    .wrapping_add(i.wrapping_mul(2_654_435_761));
                let j1 = ash_jitter(seed);
                let j2 = ash_jitter(seed.wrapping_add(11));
                let j3 = ash_jitter(seed.wrapping_add(23));

                let angle = j1 * std::f32::consts::TAU;
                let radius = ASH_SPAWN_RADIUS_MIN + ASH_SPAWN_RADIUS_SPAN * j2;
                let height = ASH_SPAWN_HEIGHT_MIN + ASH_SPAWN_HEIGHT_SPAN * j3;
                let offset =
                    Vec3::new(angle.cos() * radius, height, angle.sin() * radius);
                let speed = ASH_SHED_SPEED * (0.5 + 0.8 * j2);
                let life = ASH_LIFETIME * (0.75 + 0.5 * j3);

                commands.spawn((
                    RefusedHealMote {
                        // Outward and slightly down from the start: the mote is
                        // leaving, not arriving.
                        velocity: Vec3::new(
                            angle.cos() * speed,
                            -0.25 * speed,
                            angle.sin() * speed,
                        ),
                        lifetime: life,
                        initial_lifetime: life,
                    },
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(origin + offset),
                    PlayMatchEntity,
                ));
            }
        }
        commands.entity(signal_entity).despawn();
    }
}

/// Update (graphical-only): drift the ash outward, let it sink, shrink it out.
pub fn update_heal_fracture(
    time: Res<Time>,
    mut motes: Query<(&mut RefusedHealMote, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (mut mote, mut transform) in motes.iter_mut() {
        mote.lifetime -= dt;
        mote.velocity.y -= ASH_SINK * dt;
        let velocity = mote.velocity;
        transform.translation += velocity * dt;
        let k = (mote.lifetime / mote.initial_lifetime).clamp(0.0, 1.0);
        transform.scale = Vec3::splat(0.35 + 0.65 * k);
    }
}

/// Cleanup (graphical-only): despawn spent motes.
pub fn cleanup_heal_fracture(
    mut commands: Commands,
    motes: Query<(Entity, &RefusedHealMote)>,
) {
    for (entity, mote) in motes.iter() {
        if mote.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mote_count_scales_with_the_refused_share() {
        assert!(mote_count(0.7) > mote_count(0.2));
    }

    #[test]
    fn a_small_refusal_still_sheds_visible_ash() {
        assert_eq!(mote_count(0.0), ASH_MOTES_MIN);
        assert!(mote_count(0.05) >= ASH_MOTES_MIN);
    }

    #[test]
    fn mortal_strikes_shipped_reduction_sheds_a_readable_burst() {
        // Mortal Strike / Aimed Shot magnitude is 0.65, so 35% is refused.
        let n = mote_count(0.35);
        assert!((7..=14).contains(&n), "expected a readable burst, got {n}");
    }

    #[test]
    fn ash_jitter_is_bounded() {
        for seed in [0u32, 1, 99, 100_000] {
            let j = ash_jitter(seed);
            assert!((0.0..1.0).contains(&j), "jitter out of range: {j}");
        }
    }
}
