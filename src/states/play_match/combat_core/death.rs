//! Death animation and pet despawn systems.

use bevy::prelude::*;
use crate::combat::log::{CombatLog, CombatLogEventType};
use super::super::components::*;

/// Trigger death animation when a combatant dies.
/// Detects dead combatants without a DeathAnimation component and adds one.
pub fn trigger_death_animation(
    mut commands: Commands,
    combatants: Query<(Entity, &Combatant, &Transform), Without<DeathAnimation>>,
    all_combatants: Query<(&Transform, &Combatant)>,
) {
    for (entity, combatant, transform) in combatants.iter() {
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

        info!(
            "Team {} {} death animation started (falling toward {:?})",
            combatant.team,
            combatant.class.name(),
            fall_direction
        );
    }
}

/// Animate dead combatants falling over.
/// Updates the DeathAnimation component each frame to rotate and lower the capsule.
pub fn animate_death(
    time: Res<Time>,
    mut combatants: Query<(&mut Transform, &mut DeathAnimation)>,
) {
    let dt = time.delta_secs();

    for (mut transform, mut death_anim) in combatants.iter_mut() {
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

        if rotation_axis != Vec3::ZERO {
            let rotation_angle = t * std::f32::consts::FRAC_PI_2; // 90 degrees
            transform.rotation = Quat::from_axis_angle(rotation_axis, rotation_angle);
        }

        // Lower Y as capsule falls (1.0 standing -> 0.5 lying flat)
        transform.translation.y = 1.0 - (t * 0.5);
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
                    format!("[DEATH] Team {} {} despawns (owner died)", pet_combatant.team, pet.pet_type.name()),
                );
            }
        }
    }
}
