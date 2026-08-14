use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Fear Visual Effect System
// ==============================================================================
//
// While a unit has the Fear aura its body renders as a terror-struck husk: the
// `VisualBody`'s material is tinted to a dark, desaturated shadow-violet and
// wrapped in a soft breathing aura sphere. Fear is Shadow school. Unlike
// Polymorph this does NOT swap the mesh or spawn limb primitives — it tints in
// place and adds one owner-scoped shroud child.
//
// Keyed on `AuraType::Fear`, so Death Coil's horror (a Fear-type aura) inherits
// the treatment. Every exit path (natural expiry, damage break, dispel, death)
// restores the body, driven by the `FearedVisual` marker.

/// The shadow-violet husk tint swapped onto a feared unit's body. Dark and
/// desaturated (Shadow school) with a high roughness so it reads as drained
/// rather than lit.
const FEAR_HUSK_COLOR: Color = Color::srgb(0.28, 0.16, 0.40);
/// The breathing shroud's dark-violet additive color.
const FEAR_SHROUD_COLOR: Color = Color::srgba(0.35, 0.18, 0.55, 0.25);
/// Shroud radius as a multiple of the body extent — slightly larger than the
/// body so it reads as an aura wrapping it.
const FEAR_SHROUD_SCALE: f32 = 1.25;
/// Breathing period of the shroud, in seconds.
const FEAR_SHROUD_PERIOD: f32 = 2.0;

/// System that applies/removes the shadow-husk fear treatment.
///
/// The aura state lives on the sim entity; the material lives on its
/// [`VisualBody`] child, so this joins across the hierarchy. `ActiveAuras` is
/// OPTIONAL because `update_auras` removes the component outright once the last
/// aura expires — required for the component, not the vec, to signal the end.
///
/// Carries `Without<PolymorphedVisual>`: a unit CAN be both feared and
/// polymorphed (different DR categories; Fear deals no damage so it lands on a
/// sheep without breaking it), and the sheep look wins while polymorphed. When
/// Polymorph ends with Fear still active the exclusion lifts and this system
/// takes over the body next frame (it re-evaluates every frame).
pub fn update_fear_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    combatants: Query<
        (
            Entity,
            &Combatant,
            &Transform,
            Option<&ActiveAuras>,
            Option<&FearedVisual>,
            &Children,
        ),
        Without<PolymorphedVisual>,
    >,
    // The body child carries the material Fear tints. Fear does NOT touch the
    // mesh, so this query deliberately omits `Mesh3d`/`OriginalMesh`.
    mut bodies: Query<(
        &mut MeshMaterial3d<StandardMaterial>,
        &VisualBody,
        Option<&OriginalBodyMaterial>,
    )>,
    shroud: Query<(Entity, &FearShroud)>,
) {
    for (entity, combatant, _transform, auras, feared_marker, children) in combatants.iter() {
        // A killing blow leaves the aura ON the corpse — `update_auras` skips
        // dead combatants entirely — so death has to count as an exit path here
        // or the husk sticks to the corpse for the rest of the match.
        let is_feared = combatant.is_alive()
            && auras.is_some_and(|a| a.auras.iter().any(|au| au.effect_type == AuraType::Fear));

        if is_feared && feared_marker.is_none() {
            // Just feared. Locate the body child before allocating anything:
            // without one there is nothing to tint, so the marker stays off and
            // this branch retries next frame — and per-retry material allocation
            // would be pure asset churn.
            let Some(body_child) = children.iter().find(|&c| bodies.contains(c)) else {
                continue;
            };
            let Ok((mut material, _body, _)) = bodies.get_mut(body_child) else {
                continue;
            };

            let husk = materials.add(StandardMaterial {
                base_color: FEAR_HUSK_COLOR,
                perceptual_roughness: 0.95,
                ..default()
            });

            // Store the displaced handle so restore can put the real body
            // material back. Guarded by the `FearedVisual` marker's absence (the
            // transition-in condition), NOT by `OriginalBodyMaterial.is_none()`.
            commands
                .entity(body_child)
                .try_insert(OriginalBodyMaterial(material.0.clone()));
            *material = MeshMaterial3d(husk);

            // Breathing shroud: a slightly-larger-than-body additive sphere,
            // owner-scoped so restore despawns exactly this unit's shroud.
            let shroud_mesh = meshes.add(Sphere::new(1.0));
            let shroud_material = materials.add(StandardMaterial {
                base_color: FEAR_SHROUD_COLOR,
                emissive: LinearRgba::new(0.5, 0.25, 0.9, 1.0),
                alpha_mode: AlphaMode::Add,
                depth_bias: 0.0,
                ..default()
            });
            let shroud_entity = commands
                .spawn((
                    Mesh3d(shroud_mesh),
                    MeshMaterial3d(shroud_material),
                    Transform::from_scale(Vec3::splat(FEAR_SHROUD_SCALE)),
                    FearShroud { owner: entity },
                ))
                .id();
            commands.entity(body_child).add_child(shroud_entity);

            commands.entity(entity).try_insert(FearedVisual);
        } else if !is_feared && feared_marker.is_some() {
            // Just restored — by expiry, damage break, dispel or death. NO
            // `Without<DeathAnimation>` filter: the death sink and this restore
            // must compose in the same frame (KTD4).
            for child in children.iter() {
                if let Ok((mut material, _body, original_body_material)) = bodies.get_mut(child) {
                    if let Some(displaced) = original_body_material {
                        *material = MeshMaterial3d(displaced.0.clone());
                        commands.entity(child).remove::<OriginalBodyMaterial>();
                    }
                }
            }
            // Owner-scoped: a global sweep would strip a second unit that is
            // still feared.
            for (shroud_entity, s) in shroud.iter() {
                if s.owner == entity {
                    commands.entity(shroud_entity).despawn();
                }
            }
            commands.entity(entity).remove::<FearedVisual>();
        }
    }
}

// ==============================================================================
// Rising fear-motes emitter (R3 / KTD3)
// ==============================================================================
//
// A per-feared-unit emitter (gated on `FearedVisual`) spawns small dark-violet
// shadow motes that rise off the unit and fade, on a loop, for the aura's
// duration. Three-system shape mirroring the affliction drip emitter:
// `update_fear_mote_emitters` (tick + spawn) / `update_fear_motes` (float +
// fade) / `cleanup_fear_motes` (despawn expired). Motes are transient world
// particles — `PlayMatchEntity`-tagged, `AlphaMode::Add`, self-expiring — so
// they need no owner-scoped despawn: when `FearedVisual` is removed the emitter
// simply stops being iterated and in-flight motes finish their own lifetime.

/// Seconds between mote spawns off a feared unit.
const FEAR_MOTE_INTERVAL: f32 = 0.5;
/// A mote's lifetime, in seconds (fades to zero over this window).
const FEAR_MOTE_LIFETIME: f32 = 1.2;
/// Upward rise speed of a mote, in yards/second.
const FEAR_MOTE_RISE_SPEED: f32 = 0.9;
/// A mote's dark-violet additive color (alpha is the peak; it fades to 0).
const FEAR_MOTE_COLOR: Color = Color::srgba(0.35, 0.18, 0.55, 0.7);

/// Cheap deterministic jitter in [0,1) from a seed — visual-only, so it does
/// not touch the sim's seeded GameRng. Same hash as the affliction drips.
fn fear_mote_jitter(seed: u32) -> f32 {
    let s = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    ((s >> 9) & 0xFFFF) as f32 / 65536.0
}

/// Emitter: for each unit currently wearing `FearedVisual`, spawn a rising
/// shadow mote every `FEAR_MOTE_INTERVAL`. The interval-timer state lives on the
/// unit itself (`FearMoteEmitter`), lazily inserted the first tick a unit is
/// feared — so this one system does the affliction pattern's detector + emitter
/// work. Removing `FearedVisual` drops the unit from this query, so spawning
/// stops on restore with no despawn bookkeeping.
pub fn update_fear_mote_emitters(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut feared: Query<(Entity, &Transform, Option<&mut FearMoteEmitter>), With<FearedVisual>>,
) {
    let dt = time.delta_secs();
    for (entity, transform, emitter) in feared.iter_mut() {
        // First feared tick: attach the timer and wait for the next frame. A
        // fresh emitter avoids inheriting a stale accumulator across re-fears.
        let Some(mut emitter) = emitter else {
            commands.entity(entity).try_insert(FearMoteEmitter::default());
            continue;
        };

        emitter.spawn_accumulator += dt;
        while emitter.spawn_accumulator >= FEAR_MOTE_INTERVAL {
            emitter.spawn_accumulator -= FEAR_MOTE_INTERVAL;
            let seed = entity.index().wrapping_add(emitter.motes_spawned.wrapping_mul(7));
            emitter.motes_spawned = emitter.motes_spawned.wrapping_add(1);

            // Jittered spawn point around the torso, rising with slight drift.
            let angle = fear_mote_jitter(seed) * std::f32::consts::TAU;
            let radius = 0.20 + 0.25 * fear_mote_jitter(seed + 1);
            let height = 0.40 + 0.40 * fear_mote_jitter(seed + 2);
            let offset = Vec3::new(angle.cos() * radius, height, angle.sin() * radius);
            let drift = Vec3::new(
                (fear_mote_jitter(seed + 3) - 0.5) * 0.30,
                FEAR_MOTE_RISE_SPEED,
                (fear_mote_jitter(seed + 4) - 0.5) * 0.30,
            );

            let mesh = meshes.add(Sphere::new(0.09));
            let material = materials.add(StandardMaterial {
                base_color: FEAR_MOTE_COLOR,
                emissive: LinearRgba::new(0.40, 0.15, 0.70, 1.0),
                alpha_mode: AlphaMode::Add,
                unlit: true,
                ..default()
            });

            commands.spawn((
                FearMote {
                    velocity: drift,
                    lifetime: FEAR_MOTE_LIFETIME,
                    initial_lifetime: FEAR_MOTE_LIFETIME,
                },
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(transform.translation + offset),
                PlayMatchEntity,
            ));
        }
    }
}

/// Update: float each mote upward along its velocity and fade its alpha with
/// remaining life. Time-driven (never gated on sim movement — the
/// fixed-timestep-strobe trap), so motes rise off a stationary feared unit too.
pub fn update_fear_motes(
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut motes: Query<(&mut FearMote, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
) {
    let dt = time.delta_secs();
    for (mut mote, mut transform, material_handle) in motes.iter_mut() {
        mote.lifetime -= dt;
        transform.translation += mote.velocity * dt;

        let life_ratio = (mote.lifetime / mote.initial_lifetime).clamp(0.0, 1.0);
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let base = FEAR_MOTE_COLOR.to_srgba();
            material.base_color =
                Color::srgba(base.red, base.green, base.blue, base.alpha * life_ratio);
        }
    }
}

/// Cleanup: despawn motes whose lifetime has run out. Separate from the update
/// so the float/fade and the despawn stay single-responsibility (the plan's
/// three-system shape).
pub fn cleanup_fear_motes(mut commands: Commands, motes: Query<(Entity, &FearMote)>) {
    for (entity, mote) in motes.iter() {
        if mote.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// System that breathes each [`FearShroud`]: a slow pulse of scale and material
/// alpha over ~2s, reading as a labored terror breath. Time-driven (never gated
/// on sim movement — the fixed-timestep-strobe trap), so a stationary feared
/// unit still breathes.
pub fn update_fear_shroud(
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shrouds: Query<(&mut Transform, &MeshMaterial3d<StandardMaterial>), With<FearShroud>>,
) {
    // A 0..1 breath phase: 0 at the trough, 1 at the peak.
    let phase = 0.5
        + 0.5 * (time.elapsed_secs() * std::f32::consts::TAU / FEAR_SHROUD_PERIOD).sin();
    for (mut transform, material) in shrouds.iter_mut() {
        // Scale swells ~8% about the rest radius.
        let scale = FEAR_SHROUD_SCALE * (1.0 + 0.08 * phase);
        transform.scale = Vec3::splat(scale);
        // Alpha breathes with it so the shroud swells and glows on the inhale.
        if let Some(mat) = materials.get_mut(&material.0) {
            let base = FEAR_SHROUD_COLOR.to_srgba();
            mat.base_color = Color::srgba(base.red, base.green, base.blue, 0.18 + 0.14 * phase);
        }
    }
}
