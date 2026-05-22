//! Player selection — click a combatant to select it.
//!
//! Selection is purely a player affordance. A click on a combatant sets
//! [`Selection`] to that entity; a click on empty space clears it. The visual
//! is a translucent cyan-white torus laid flat at the unit's feet (the
//! [`SelectionRing`] entity).
//!
//! All systems here are graphical-only. Headless mode never registers them.
//! Registration lives in `src/states/mod.rs` (`StatesPlugin::build()`).

use bevy::prelude::*;
use bevy::color::LinearRgba;

use super::components::{ArenaCamera, Combatant, CameraController, PlayMatchEntity, SelectionRing, WalkAnim};

// =============================================================================
// Tunables
// =============================================================================

/// Maximum cursor travel (in pixels) between left-button press and release for
/// the gesture to count as a click rather than a drag.
pub const SELECTION_CLICK_THRESHOLD_PX: f32 = 5.0;

/// Maximum 2D screen-space distance (in pixels) from the cursor to a
/// combatant's projected center for that combatant to be a valid pick target.
/// Generous so the small capsules are easy to hit at default camera distance.
pub const SELECTION_PICK_RADIUS_PX: f32 = 40.0;

/// Inner radius of the selection ring torus mesh.
const RING_INNER_RADIUS: f32 = 0.75;
/// Outer radius of the selection ring torus mesh.
const RING_OUTER_RADIUS: f32 = 0.95;

/// Y offset from the combatant transform (capsule center, ~y=1.0) down to the
/// arena floor with a small lift to avoid Z-fighting with the floor plane.
const RING_GROUND_OFFSET_Y: f32 = -0.95 + 0.05;

// =============================================================================
// Resource
// =============================================================================

/// The currently selected combatant, or `None` when nothing is selected.
///
/// Updated by [`pick_selected_combatant`]. Consumed by [`sync_selection_ring`]
/// (spawns/despawns the ring entity) and [`follow_selection_ring`] (follows
/// the target and clears the selection if the target dies).
#[derive(Resource, Default)]
pub struct Selection {
    pub entity: Option<Entity>,
}

// =============================================================================
// Click vs. drag helper (testable in isolation)
// =============================================================================

/// Returns true when a press/release pair counts as a click (small cursor
/// travel) rather than a drag.
pub fn is_click_gesture(press: Vec2, release: Vec2, threshold_px: f32) -> bool {
    press.distance(release) < threshold_px
}

// =============================================================================
// Picking helper (testable in isolation)
// =============================================================================

/// Picks the closest entity in `candidates` to `cursor` (2D screen-space),
/// requiring the distance to be within `radius_px`.
///
/// `candidates` is `(entity, projected_position)`. Returns `None` when no
/// candidate is within `radius_px`.
pub fn find_closest_pick(
    cursor: Vec2,
    candidates: &[(Entity, Vec2)],
    radius_px: f32,
) -> Option<Entity> {
    let mut best: Option<(Entity, f32)> = None;
    for &(entity, projected) in candidates {
        let dist = cursor.distance(projected);
        if dist > radius_px {
            continue;
        }
        match best {
            Some((_, best_dist)) if dist >= best_dist => {}
            _ => best = Some((entity, dist)),
        }
    }
    best.map(|(entity, _)| entity)
}

// =============================================================================
// Systems
// =============================================================================

/// Reads the camera controller's pending-pick flag, runs screen-space picking
/// against alive combatants, and updates [`Selection`].
///
/// Runs every frame but short-circuits when no pick is pending.
pub fn pick_selected_combatant(
    mut camera_controller: ResMut<CameraController>,
    cameras: Query<(&Camera, &GlobalTransform), With<ArenaCamera>>,
    windows: Query<&Window>,
    combatants: Query<(Entity, &Transform, &Combatant)>,
    mut selection: ResMut<Selection>,
) {
    if !camera_controller.pending_pick {
        return;
    }
    camera_controller.pending_pick = false;

    let Ok((camera, camera_transform)) = cameras.get_single() else {
        return;
    };
    let Ok(window) = windows.get_single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    let mut projected: Vec<(Entity, Vec2)> = Vec::new();
    for (entity, transform, combatant) in combatants.iter() {
        if !combatant.is_alive() {
            continue;
        }
        if let Ok(screen) = camera.world_to_viewport(camera_transform, transform.translation) {
            projected.push((entity, screen));
        }
    }

    selection.entity = find_closest_pick(cursor, &projected, SELECTION_PICK_RADIUS_PX);
}

/// Spawns the [`SelectionRing`] entity when [`Selection`] changes to a new
/// target, despawns it when selection clears, and swaps it cleanly when the
/// target changes.
pub fn sync_selection_ring(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    selection: Res<Selection>,
    existing_rings: Query<Entity, With<SelectionRing>>,
) {
    if !selection.is_changed() {
        return;
    }

    for entity in existing_rings.iter() {
        commands.entity(entity).despawn_recursive();
    }

    let Some(target) = selection.entity else {
        return;
    };

    let mesh = meshes.add(Torus::new(RING_INNER_RADIUS, RING_OUTER_RADIUS));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.6, 0.9, 1.0, 0.6),
        emissive: LinearRgba::new(0.4, 0.8, 1.2, 1.0),
        alpha_mode: AlphaMode::Add,
        ..default()
    });

    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::default(),
        SelectionRing { target },
        PlayMatchEntity,
    ));
}

/// Each frame, position the ring at the target's feet and apply a gentle
/// pulse. Despawns the ring (and clears [`Selection`]) when the target dies
/// or is no longer a valid combatant.
pub fn follow_selection_ring(
    mut commands: Commands,
    time: Res<Time>,
    mut selection: ResMut<Selection>,
    combatants: Query<(&Transform, &Combatant, Option<&WalkAnim>), Without<SelectionRing>>,
    mut rings: Query<(Entity, &SelectionRing, &mut Transform), Without<Combatant>>,
) {
    for (ring_entity, ring, mut ring_transform) in rings.iter_mut() {
        match combatants.get(ring.target) {
            Ok((target_transform, combatant, walk)) if combatant.is_alive() => {
                // Ground-lock the ring: read logical Y from WalkAnim::ground_y when
                // present so the ring stays planted while the unit's mesh bobs.
                // Falls back to translation.y for entities without WalkAnim.
                let logical_y = walk.map(|w| w.ground_y).unwrap_or(target_transform.translation.y);
                ring_transform.translation = Vec3::new(
                    target_transform.translation.x,
                    logical_y + RING_GROUND_OFFSET_Y,
                    target_transform.translation.z,
                );
                // Bevy's Torus mesh is already flat in the XZ plane (major
                // circle in XZ, tube cross-section along Y). No rotation
                // needed.

                let pulse = 1.0 + 0.05 * (time.elapsed_secs() * 3.0).sin();
                ring_transform.scale = Vec3::splat(pulse);
            }
            _ => {
                commands.entity(ring_entity).despawn_recursive();
                if selection.entity == Some(ring.target) {
                    selection.entity = None;
                }
            }
        }
    }
}

/// Resets the selection when leaving a match so the next match starts clean.
pub fn reset_selection_on_exit(mut selection: ResMut<Selection>) {
    selection.entity = None;
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_click_gesture_within_threshold() {
        assert!(is_click_gesture(Vec2::new(100.0, 100.0), Vec2::new(102.0, 99.0), 5.0));
    }

    #[test]
    fn is_click_gesture_motionless_is_click() {
        // The motionless case — pressed and released without moving — is the
        // most common click and must register.
        assert!(is_click_gesture(Vec2::ZERO, Vec2::ZERO, 5.0));
    }

    #[test]
    fn is_click_gesture_at_threshold_is_drag() {
        // Strict less-than: exactly the threshold counts as a drag.
        assert!(!is_click_gesture(Vec2::new(0.0, 0.0), Vec2::new(5.0, 0.0), 5.0));
    }

    #[test]
    fn is_click_gesture_over_threshold_is_drag() {
        assert!(!is_click_gesture(Vec2::new(100.0, 100.0), Vec2::new(200.0, 100.0), 5.0));
    }

    #[test]
    fn find_closest_pick_returns_none_when_empty() {
        assert_eq!(find_closest_pick(Vec2::ZERO, &[], 40.0), None);
    }

    #[test]
    fn find_closest_pick_returns_none_when_all_outside_radius() {
        let a = Entity::from_raw(1);
        let candidates = [(a, Vec2::new(100.0, 100.0))];
        assert_eq!(find_closest_pick(Vec2::ZERO, &candidates, 40.0), None);
    }

    #[test]
    fn find_closest_pick_picks_only_candidate_inside_radius() {
        let a = Entity::from_raw(1);
        let candidates = [(a, Vec2::new(5.0, 5.0))];
        assert_eq!(find_closest_pick(Vec2::ZERO, &candidates, 40.0), Some(a));
    }

    #[test]
    fn find_closest_pick_picks_closest_of_multiple() {
        let a = Entity::from_raw(1);
        let b = Entity::from_raw(2);
        // Cursor at (0, 0). a is at distance 10, b is at distance 5 — b wins.
        let candidates = [(a, Vec2::new(10.0, 0.0)), (b, Vec2::new(5.0, 0.0))];
        assert_eq!(find_closest_pick(Vec2::ZERO, &candidates, 40.0), Some(b));
    }

    #[test]
    fn find_closest_pick_ignores_far_candidate_when_closer_inside() {
        let a = Entity::from_raw(1);
        let b = Entity::from_raw(2);
        // a inside radius, b outside.
        let candidates = [(a, Vec2::new(10.0, 0.0)), (b, Vec2::new(100.0, 0.0))];
        assert_eq!(find_closest_pick(Vec2::ZERO, &candidates, 40.0), Some(a));
    }

    #[test]
    fn selection_default_is_none() {
        let selection = Selection::default();
        assert!(selection.entity.is_none());
    }

    #[test]
    fn selection_ring_can_be_constructed() {
        let ring = SelectionRing { target: Entity::from_raw(7) };
        assert_eq!(ring.target, Entity::from_raw(7));
    }
}
