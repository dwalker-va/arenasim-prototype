//! Death animation and pet despawn systems.

use bevy::prelude::*;
use crate::combat::log::{CombatLog, CombatLogEventType};
use super::super::components::*;
use super::super::utils::pet_combatant_id;

/// Trigger death animation when a combatant dies.
/// Detects dead combatants without a DeathAnimation component and adds one.
pub fn trigger_death_animation(
    mut commands: Commands,
    combatants: Query<(Entity, &Combatant, &Transform, Option<&Pet>), Without<DeathAnimation>>,
    all_combatants: Query<(&Transform, &Combatant)>,
) {
    for (entity, combatant, transform, pet) in combatants.iter() {
        if combatant.is_alive() {
            continue;
        }

        // Combatant just died - calculate fall direction
        // Fall away from the nearest living enemy (dramatic effect)
        let my_pos = transform.translation;
        let mut nearest_enemy_pos: Option<Vec3> = None;
        let mut nearest_distance = f32::MAX;

        for (other_transform, other_combatant) in all_combatants.iter() {
            if other_combatant.team != combatant.team && other_combatant.is_alive() {
                let distance = my_pos.distance(other_transform.translation);
                if distance < nearest_distance {
                    nearest_distance = distance;
                    nearest_enemy_pos = Some(other_transform.translation);
                }
            }
        }

        // Fall direction: away from nearest enemy, or forward if no enemy found
        let fall_direction = if let Some(enemy_pos) = nearest_enemy_pos {
            Vec3::new(
                my_pos.x - enemy_pos.x,
                0.0,
                my_pos.z - enemy_pos.z,
            ).normalize_or_zero()
        } else {
            // No enemy found, fall in the direction they're facing
            let forward = transform.rotation * Vec3::Z;
            Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero()
        };

        // Default to falling along negative Z if no direction could be determined
        let fall_direction = if fall_direction == Vec3::ZERO {
            Vec3::new(0.0, 0.0, -1.0)
        } else {
            fall_direction
        };

        commands.entity(entity).insert(DeathAnimation::new(fall_direction));

        let display_name = pet.map_or_else(|| combatant.class.name(), |p| p.pet_type.name());
        info!(
            "Team {} {} death animation started (falling toward {:?})",
            combatant.team,
            display_name,
            fall_direction
        );
    }
}

/// Animate dead combatants falling over.
///
/// Purely visual: it drives the corpse's [`VisualBody`] child, never the sim
/// entity's own `Transform`. Gameplay range checks read the parent's translation
/// (including `y`), so animating the parent would feed a graphical effect back
/// into the simulation — see `VisualBody`.
pub fn animate_death(
    time: Res<Time>,
    mut combatants: Query<(&mut DeathAnimation, &Children)>,
    mut bodies: Query<(&mut Transform, &VisualBody)>,
) {
    let dt = time.delta_secs();

    for (mut death_anim, children) in combatants.iter_mut() {
        if death_anim.is_complete() {
            continue;
        }

        // Advance animation
        death_anim.progress += dt / DeathAnimation::DURATION;
        death_anim.progress = death_anim.progress.min(1.0);

        // Ease-out for natural deceleration (fast start, slow finish)
        let t = ease_out_quad(death_anim.progress);

        // Rotation: 0° -> 90° around axis perpendicular to fall direction
        // The rotation axis is perpendicular to both Y (up) and fall direction
        let rotation_axis = Vec3::Y.cross(death_anim.fall_direction).normalize_or_zero();

        for child in children.iter() {
            let Ok((mut body_transform, body)) = bodies.get_mut(child) else {
                continue;
            };
            if rotation_axis != Vec3::ZERO {
                let rotation_angle = t * std::f32::consts::FRAC_PI_2; // 90 degrees
                body_transform.rotation = Quat::from_axis_angle(rotation_axis, rotation_angle);
            }
            // Sink as the capsule falls: 0.5 units below its resting height,
            // which reproduces the old absolute 1.0 -> 0.5 for a combatant
            // standing at y = 1.0 without hardcoding that standing height.
            body_transform.translation.y = body.rest_y - t * 0.5;
        }
    }
}

/// Ease-out quadratic function for smooth deceleration.
/// Returns 0.0 at t=0.0 and 1.0 at t=1.0, with decreasing rate of change.
fn ease_out_quad(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(2)
}

/// Test-only access to ease_out_quad.
#[cfg(test)]
pub fn ease_out_quad_for_test(t: f32) -> f32 {
    ease_out_quad(t)
}

/// Despawn pets whose owner has died by setting their HP to 0.
pub fn despawn_pets_of_dead_owners(
    mut combat_log: ResMut<CombatLog>,
    mut pets: Query<(Entity, &Pet, &mut Combatant)>,
    owners: Query<&Combatant, Without<Pet>>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    if celebration.is_some() { return; }
    for (_pet_entity, pet, mut pet_combatant) in pets.iter_mut() {
        if !pet_combatant.is_alive() {
            continue;
        }
        if let Ok(owner) = owners.get(pet.owner) {
            if !owner.is_alive() {
                pet_combatant.current_health = 0.0;
                combat_log.log(
                    CombatLogEventType::Death,
                    format!("{} despawns (owner died)", pet_combatant_id(pet_combatant.team, pet_combatant.owner_relative_slot(), pet.pet_type)),
                );
            }
        }
    }
}
