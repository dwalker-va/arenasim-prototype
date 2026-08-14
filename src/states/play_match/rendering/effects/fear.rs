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
