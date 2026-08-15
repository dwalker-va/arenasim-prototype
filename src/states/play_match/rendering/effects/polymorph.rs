use bevy::prelude::*;
use crate::states::play_match::components::*;
use super::transform_puffs::spawn_transform_puff;

// ==============================================================================
// Polymorph Visual Effect System
// ==============================================================================

/// Half-extents of the sheep torso, in the [`VisualBody`]'s local space. These
/// reproduce the footprint of the cuboid placeholder the sheep replaced
/// (0.8 x 0.6 x 1.0), so the transformed unit occupies the same volume it did.
const SHEEP_TORSO_HALF: Vec3 = Vec3::new(0.40, 0.30, 0.50);
/// Leg length; also the torso's clearance above the floor.
const SHEEP_LEG_LEN: f32 = 0.30;
const SHEEP_HEAD_RADIUS: f32 = 0.22;
/// Wool: off-white and fully rough, so the sheep reads as fleece rather than as
/// a lit surface next to the glossy class capsules.
const SHEEP_WOOL_COLOR: Color = Color::srgb(0.93, 0.91, 0.85);
/// Face, ears and legs — the bare-skin parts.
const SHEEP_SKIN_COLOR: Color = Color::srgb(0.30, 0.26, 0.26);

/// System that swaps combatant meshes when polymorphed.
///
/// The victim's body becomes a sheep: the [`VisualBody`]'s own mesh is swapped
/// to the wool torso, and the head, ears, legs and tail ride along as
/// [`SheepPart`] children of it (so they inherit the walk bob and despawn with
/// the unit for free). Weapon sockets are hidden separately, by
/// `animate_weapon_swings`.
pub fn update_polymorph_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // The aura state lives on the sim entity; the mesh lives on its `VisualBody`
    // child, so this joins across the hierarchy. `ActiveAuras` is OPTIONAL
    // because `update_auras` removes the component outright once the last aura
    // expires — required for the component, not the vec, to signal the end.
    // `Without<FearedVisual>` makes the two body treatments mutually exclusive:
    // whichever CC's visual grabs the body first holds the single
    // `OriginalBodyMaterial` slot until it lifts, then the other applies. Without
    // this, a Fear-then-Polymorph sequence has Polymorph overwrite the real
    // material handle Fear stored, leaving the unit stuck on the husk tint.
    // (Fear's query carries the mirror `Without<PolymorphedVisual>`.)
    combatants: Query<
        (
            Entity,
            &Combatant,
            &Transform,
            Option<&ActiveAuras>,
            Option<&PolymorphedVisual>,
            &Children,
        ),
        Without<FearedVisual>,
    >,
    mut bodies: Query<(
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
        &OriginalMesh,
        &VisualBody,
        Option<&OriginalBodyMaterial>,
    )>,
    parts: Query<(Entity, &SheepPart)>,
) {
    for (entity, combatant, transform, auras, polymorphed_marker, children) in combatants.iter() {
        // A killing blow leaves the aura ON the corpse — `update_auras` skips
        // dead combatants entirely — so death has to count as an exit path here
        // or the loser stays a sheep for the rest of the match.
        let is_polymorphed = combatant.is_alive()
            && auras.is_some_and(|a| {
                a.auras.iter().any(|au| au.effect_type == AuraType::Polymorph)
            });

        if is_polymorphed && polymorphed_marker.is_none() {
            // Just transformed. Locate the body child before allocating
            // anything: without one there is nothing to restore, so the marker
            // stays off and this branch retries next frame — and per-retry
            // material allocation would be pure asset churn.
            let Some(body_child) = children.iter().find(|&c| bodies.contains(c)) else {
                continue;
            };
            let Ok((mut mesh3d, mut material, _, body, _)) = bodies.get_mut(body_child) else {
                continue;
            };

            let wool = materials.add(StandardMaterial {
                base_color: SHEEP_WOOL_COLOR,
                perceptual_roughness: 1.0,
                ..default()
            });
            let skin = materials.add(StandardMaterial {
                base_color: SHEEP_SKIN_COLOR,
                perceptual_roughness: 0.9,
                ..default()
            });

            // The body's world rest height. Derived rather than hardcoded
            // because pets render their body at an offset from the sim
            // entity's `y` (see `VisualBody::rest_y`). Negated, it is the
            // floor height in the body's local space.
            let body_rest_world_y = transform.translation.y + body.rest_y;
            let ground_y = -body_rest_world_y;
            let torso_y = ground_y + SHEEP_LEG_LEN + SHEEP_TORSO_HALF.y;

            // The torso is the body's OWN mesh, whose transform belongs to
            // the walk bob and the death sink, so its offset and squash are
            // baked into the mesh instead of applied as a scale.
            *mesh3d = Mesh3d(meshes.add(
                Sphere::new(1.0)
                    .mesh()
                    .uv(24, 12)
                    .scaled_by(SHEEP_TORSO_HALF)
                    .translated_by(Vec3::Y * torso_y),
            ));
            commands
                .entity(body_child)
                .try_insert(OriginalBodyMaterial(material.0.clone()));
            *material = MeshMaterial3d(wool.clone());

            spawn_sheep_parts(
                &mut commands,
                &mut meshes,
                &wool,
                &skin,
                body_child,
                entity,
                ground_y,
                torso_y,
            );
            commands.entity(entity).try_insert(PolymorphedVisual);
            spawn_transform_puff(&mut commands, transform.translation, Some(body_rest_world_y));
        } else if !is_polymorphed && polymorphed_marker.is_some() {
            // Just restored — by expiry, damage break, dispel or death. Death is
            // one of those paths, so this doubles as the death-break puff.
            let mut torso_world_y = None;
            for child in children.iter() {
                if let Ok((mut mesh3d, mut material, original_mesh, body, original_body_material)) =
                    bodies.get_mut(child)
                {
                    *mesh3d = Mesh3d(original_mesh.0.clone());
                    if let Some(displaced) = original_body_material {
                        *material = MeshMaterial3d(displaced.0.clone());
                        commands.entity(child).remove::<OriginalBodyMaterial>();
                    }
                    torso_world_y = Some(transform.translation.y + body.rest_y);
                }
            }
            spawn_transform_puff(&mut commands, transform.translation, torso_world_y);
            // Owner-scoped: a global sweep would strip a second sheep that is
            // still polymorphed.
            for (part_entity, part) in parts.iter() {
                if part.owner == entity {
                    commands.entity(part_entity).despawn();
                }
            }
            commands.entity(entity).remove::<PolymorphedVisual>();
        }
    }
}

/// Spawn the sheep's non-torso primitives as children of `body`, tagged for
/// `owner` so restore despawns exactly this unit's set.
///
/// `ground_y` and `torso_y` are local heights in the body's space: the floor and
/// the torso's centre.
#[allow(clippy::too_many_arguments)]
fn spawn_sheep_parts(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    wool: &Handle<StandardMaterial>,
    skin: &Handle<StandardMaterial>,
    body: Entity,
    owner: Entity,
    ground_y: f32,
    torso_y: f32,
) {
    // One unit sphere, posed per part by the child transforms.
    let ball = meshes.add(Sphere::new(1.0).mesh().uv(16, 10));
    // Legs run from the floor up INTO the torso's interior, not just to its
    // lowest plane: the torso is an ellipsoid, so at the corners where the
    // legs sit its underside curves ~0.14 above the bottom — a leg stopping
    // at the bottom plane floats visibly disconnected.
    let leg_len = SHEEP_LEG_LEN + SHEEP_TORSO_HALF.y;
    let leg = meshes.add(Cylinder::new(0.06, leg_len));

    let head_y = torso_y + 0.14;
    let head_z = SHEEP_TORSO_HALF.z * 0.85;
    let mut part = |mesh: Handle<Mesh>, material: Handle<StandardMaterial>, transform: Transform| {
        let child = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                transform,
                SheepPart { owner },
            ))
            .id();
        commands.entity(body).add_child(child);
    };

    // Head, with a darker muzzle poking out of the fleece.
    part(
        ball.clone(),
        wool.clone(),
        Transform::from_xyz(0.0, head_y, head_z).with_scale(Vec3::splat(SHEEP_HEAD_RADIUS)),
    );
    part(
        ball.clone(),
        skin.clone(),
        Transform::from_xyz(0.0, head_y - 0.06, head_z + 0.16)
            .with_scale(Vec3::new(0.11, 0.09, 0.13)),
    );
    // Ears: flat lozenges angled up and out, one per side.
    for side in [-1.0f32, 1.0] {
        part(
            ball.clone(),
            skin.clone(),
            Transform::from_xyz(side * 0.19, head_y + 0.08, head_z - 0.06)
                .with_rotation(Quat::from_rotation_z(side * 0.5))
                .with_scale(Vec3::new(0.14, 0.04, 0.08)),
        );
    }
    // Legs at the corners of the torso footprint; the cylinder's origin is its
    // middle, so centring at half a leg up keeps the feet exactly on `ground_y`
    // while the top half embeds in the torso.
    for (x, z) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        part(
            leg.clone(),
            skin.clone(),
            Transform::from_xyz(
                x * SHEEP_TORSO_HALF.x * 0.6,
                ground_y + leg_len * 0.5,
                z * SHEEP_TORSO_HALF.z * 0.6,
            ),
        );
    }
    // Tail.
    part(
        ball,
        wool.clone(),
        Transform::from_xyz(0.0, torso_y + 0.12, -SHEEP_TORSO_HALF.z - 0.02)
            .with_scale(Vec3::new(0.09, 0.11, 0.09)),
    );
}

