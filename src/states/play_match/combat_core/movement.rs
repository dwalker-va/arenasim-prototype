//! Movement systems: target following, kiting, fear/polymorph wandering, charging, disengaging.

use bevy::prelude::*;
use super::super::components::*;
use super::{is_in_arena_bounds, clamp_to_arena};
use super::super::{MELEE_RANGE, DISENGAGE_SPEED};

fn find_best_kiting_direction(
    current_pos: Vec3,
    enemy_pos: Vec3,
    move_distance: f32,
) -> Vec3 {
    // Calculate ideal direction (directly away from enemy)
    let ideal_direction = Vec3::new(
        current_pos.x - enemy_pos.x,
        0.0,
        current_pos.z - enemy_pos.z,
    ).normalize_or_zero();

    if ideal_direction == Vec3::ZERO {
        return Vec3::ZERO; // Already on top of enemy, can't kite
    }

    // Check if ideal direction keeps us in bounds
    let ideal_next_pos = current_pos + ideal_direction * move_distance;
    let ideal_in_bounds = is_in_arena_bounds(ideal_next_pos);

    if ideal_in_bounds {
        return ideal_direction; // Ideal direction works, use it!
    }

    // Ideal direction would hit boundary - find best alternative
    // Test 16 directions around a circle and pick the one that:
    // 1. Stays in bounds
    // 2. Maximizes distance from enemy
    let mut best_direction = Vec3::ZERO;
    let mut best_score = f32::MIN;

    for i in 0..16 {
        let angle = (i as f32) * std::f32::consts::TAU / 16.0;
        let candidate_direction = Vec3::new(
            angle.cos(),
            0.0,
            angle.sin(),
        );

        // Calculate where we'd end up with this direction
        let candidate_next_pos = current_pos + candidate_direction * move_distance;

        // Check if this keeps us in bounds
        let in_bounds = is_in_arena_bounds(candidate_next_pos);

        if !in_bounds {
            continue; // Skip directions that go out of bounds
        }

        // Score this direction based on:
        // 1. Distance from enemy (higher = better)
        // 2. Alignment with ideal direction (bonus for moving away, not sideways)
        let distance_from_enemy = candidate_next_pos.distance(enemy_pos);
        let alignment_with_ideal = candidate_direction.dot(ideal_direction).max(0.0);
        let center_dist = Vec3::new(candidate_next_pos.x, 0.0, candidate_next_pos.z).length();
        let center_bonus = (40.0 - center_dist).max(0.0) * 0.1;
        let score = distance_from_enemy * 2.0 + alignment_with_ideal * 5.0 + center_bonus;

        if score > best_score {
            best_score = score;
            best_direction = candidate_direction;
        }
    }

    best_direction
}

pub fn move_to_target(
    countdown: Res<MatchCountdown>,
    time: Res<Time>,
    mut commands: Commands,
    mut combatants: Query<(Entity, &mut Transform, &Combatant, Option<&ActiveAuras>, Option<&CastingState>, Option<&ChargingState>, Option<&ChannelingState>, Option<&DisengagingState>)>,
    orbs: Query<&Transform, (With<ShadowSightOrb>, Without<Combatant>)>,
    pet_query: Query<&Pet>,
) {
    // Don't allow movement until gates open
    if !countdown.gates_opened {
        return;
    }

    let dt = time.delta_secs();

    // Build a snapshot of all combatant positions and team info for lookups
    let positions: std::collections::HashMap<Entity, (Vec3, u8)> = combatants
        .iter()
        .map(|(entity, transform, combatant, _, _, _, _, _)| (entity, (transform.translation, combatant.team)))
        .collect();

    // Move each combatant towards their target if needed
    for (entity, mut transform, combatant, auras, casting_state, charging_state, channeling_state, disengaging_state) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }

        // Cannot move while casting (WoW mechanic)
        if casting_state.is_some() {
            continue;
        }

        // Cannot move while channeling (WoW mechanic)
        if channeling_state.is_some() {
            continue;
        }

        // Check for movement-preventing CC and wandering CC
        let (is_rooted_or_stunned, fear_direction, polymorph_direction) = if let Some(auras) = auras {
            let rooted_or_stunned = auras.auras.iter().any(|a| matches!(a.effect_type, AuraType::Root | AuraType::Stun | AuraType::Incapacitate));
            let fear_dir = auras.auras.iter()
                .find(|a| a.effect_type == AuraType::Fear)
                .map(|a| a.fear_direction);
            let poly_dir = auras.auras.iter()
                .find(|a| a.effect_type == AuraType::Polymorph)
                .map(|a| a.fear_direction); // Polymorph reuses fear_direction for wandering
            (rooted_or_stunned, fear_dir, poly_dir)
        } else {
            (false, None, None)
        };

        // Cannot move at all if rooted or stunned
        if is_rooted_or_stunned {
            continue;
        }

        let my_pos = transform.translation;

        // FEARED BEHAVIOR: If feared, run in random direction (ignoring normal movement)
        if let Some((dir_x, dir_z)) = fear_direction {
            let direction = Vec3::new(dir_x, 0.0, dir_z).normalize_or_zero();

            if direction != Vec3::ZERO {
                // Feared targets run at normal movement speed (no slows applied during fear)
                let move_distance = combatant.base_movement_speed * dt;

                // Move in fear direction
                transform.translation += direction * move_distance;

                // Clamp to arena bounds
                transform.translation = clamp_to_arena(transform.translation);

                // Rotate to face direction of travel
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }

            continue; // Skip normal movement logic while feared
        }

        // POLYMORPH BEHAVIOR: If polymorphed, wander slowly (like Fear but at 50% speed)
        if let Some((dir_x, dir_z)) = polymorph_direction {
            let direction = Vec3::new(dir_x, 0.0, dir_z).normalize_or_zero();

            if direction != Vec3::ZERO {
                // Polymorphed targets wander at 20% of normal movement speed (sheep waddle slowly!)
                let move_distance = combatant.base_movement_speed * 0.2 * dt;

                // Move in polymorph direction
                transform.translation += direction * move_distance;

                // Clamp to arena bounds
                transform.translation = clamp_to_arena(transform.translation);

                // Rotate to face direction of travel
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }

            continue; // Skip normal movement logic while polymorphed
        }

        // CHARGING BEHAVIOR: If charging, move at high speed toward target ignoring slows
        if let Some(charge_state) = charging_state {
            let Some(&(target_pos, _)) = positions.get(&charge_state.target) else {
                // Target doesn't exist, cancel charge
                commands.entity(entity).remove::<ChargingState>();
                continue;
            };

            let distance = my_pos.distance(target_pos);

            // If we've reached melee range, end the charge
            if distance <= MELEE_RANGE {
                commands.entity(entity).remove::<ChargingState>();

                info!(
                    "Team {} {} completes charge!",
                    combatant.team,
                    combatant.class.name()
                );

                continue; // Will use normal movement/combat next frame
            }

            // Calculate direction to target
            let direction = Vec3::new(
                target_pos.x - my_pos.x,
                0.0,
                target_pos.z - my_pos.z,
            ).normalize_or_zero();

            if direction != Vec3::ZERO {
                // Charge speed: 4x normal movement speed, ignores slows
                const CHARGE_SPEED_MULTIPLIER: f32 = 4.0;
                let charge_speed = combatant.base_movement_speed * CHARGE_SPEED_MULTIPLIER;
                let move_distance = charge_speed * dt;

                // Move towards target
                transform.translation += direction * move_distance;

                // Clamp position to arena bounds
                transform.translation = clamp_to_arena(transform.translation);

                // Rotate to face target
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }

            continue; // Skip normal movement logic while charging
        }

        // DISENGAGE BEHAVIOR: If disengaging, leap backward at high speed
        if let Some(disengage) = disengaging_state {
            if disengage.distance_remaining > 0.0 {
                let move_amount = DISENGAGE_SPEED * dt;
                let new_pos = transform.translation + disengage.direction * move_amount;

                // Clamp to arena bounds
                transform.translation = clamp_to_arena(new_pos);

                // Decrement distance remaining
                let remaining = disengage.distance_remaining - move_amount;
                if remaining <= 0.0 {
                    commands.entity(entity).remove::<DisengagingState>();
                } else {
                    commands.entity(entity).try_insert(DisengagingState {
                        direction: disengage.direction,
                        distance_remaining: remaining,
                    });
                }

                continue; // Skip normal movement logic while disengaging
            } else {
                commands.entity(entity).remove::<DisengagingState>();
            }
        }

        // KITING BEHAVIOR: If kiting_timer > 0, move away from nearest enemy
        // Uses intelligent pathfinding that considers arena boundaries
        if combatant.kiting_timer > 0.0 {
            // Find nearest enemy
            let mut nearest_enemy_pos: Option<Vec3> = None;
            let mut nearest_distance = f32::MAX;

            for (other_entity, &(other_pos, other_team)) in positions.iter() {
                if *other_entity != entity && other_team != combatant.team {
                    let distance = my_pos.distance(other_pos);
                    if distance < nearest_distance {
                        nearest_distance = distance;
                        nearest_enemy_pos = Some(other_pos);
                    }
                }
            }

            // Intelligent kiting: maximize distance from nearest enemy
            if let Some(enemy_pos) = nearest_enemy_pos {
                // Calculate effective movement speed (base * aura modifiers)
                let mut movement_speed = combatant.base_movement_speed;
                if let Some(auras) = auras {
                    for aura in &auras.auras {
                        if aura.effect_type == AuraType::MovementSpeedSlow {
                            movement_speed *= aura.magnitude;
                        }
                    }
                }

                let move_distance = movement_speed * dt;

                // Find the best direction to move that maximizes distance from enemy
                // while staying within arena bounds
                let best_direction = find_best_kiting_direction(
                    my_pos,
                    enemy_pos,
                    move_distance,
                );

                if best_direction != Vec3::ZERO {
                    // Move in the best direction
                    transform.translation += best_direction * move_distance;

                    // Ensure we stay in bounds (in case of floating point errors)
                    transform.translation = clamp_to_arena(transform.translation);

                    // Rotate to face direction of travel
                    let target_rotation = Quat::from_rotation_y(best_direction.x.atan2(best_direction.z));
                    transform.rotation = target_rotation;
                }
            }

            continue; // Skip normal movement logic
        }

        // NORMAL MOVEMENT: Get target position
        let Some(target_entity) = combatant.target else {
            // Pets with no target follow their owner
            if let Ok(pet) = pet_query.get(entity) {
                if let Some(&(owner_pos, _)) = positions.get(&pet.owner) {
                    let dist_to_owner = my_pos.distance(owner_pos);
                    if dist_to_owner > 3.0 {
                        let direction = Vec3::new(
                            owner_pos.x - my_pos.x, 0.0, owner_pos.z - my_pos.z,
                        ).normalize_or_zero();
                        if direction != Vec3::ZERO {
                            let mut movement_speed = combatant.base_movement_speed;
                            if let Some(auras) = auras {
                                for aura in &auras.auras {
                                    if aura.effect_type == AuraType::MovementSpeedSlow {
                                        movement_speed *= aura.magnitude;
                                    }
                                }
                            }
                            let move_distance = movement_speed * dt;
                            transform.translation += direction * move_distance;
                            transform.translation = clamp_to_arena(transform.translation);
                            let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                            transform.rotation = target_rotation;
                        }
                    }
                }
                continue;
            }

            // No target available (likely facing all-stealth team)
            // When orbs are spawned, ALL combatants (even stealthed) should seek them
            // to break the stalemate. This represents accepting the reveal to gain vision.

            // Find nearest Shadow Sight orb (if any exist)
            let nearest_orb_pos = orbs.iter()
                .map(|orb_transform| orb_transform.translation)
                .min_by(|a, b| {
                    let dist_a = my_pos.distance(*a);
                    let dist_b = my_pos.distance(*b);
                    dist_a.partial_cmp(&dist_b).unwrap()
                });

            // Determine destination: nearest orb if available, otherwise center
            let destination = nearest_orb_pos.unwrap_or(Vec3::ZERO);
            let distance_to_destination = my_pos.distance(destination);

            // Only move if we're far from destination (> 2.5 units for orbs, > 5 units for center)
            let stop_distance = if nearest_orb_pos.is_some() { 2.5 } else { 5.0 };
            if distance_to_destination > stop_distance {
                let direction = Vec3::new(
                    destination.x - my_pos.x,
                    0.0,
                    destination.z - my_pos.z,
                ).normalize_or_zero();

                if direction != Vec3::ZERO {
                    // Calculate effective movement speed
                    let mut movement_speed = combatant.base_movement_speed;
                    if let Some(auras) = auras {
                        for aura in &auras.auras {
                            if aura.effect_type == AuraType::MovementSpeedSlow {
                                movement_speed *= aura.magnitude;
                            }
                        }
                    }

                    // Move towards destination
                    let move_distance = movement_speed * dt;
                    transform.translation += direction * move_distance;

                    // Clamp position to arena bounds
                    transform.translation = clamp_to_arena(transform.translation);

                    // Rotate to face destination
                    let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                    transform.rotation = target_rotation;
                }
            }

            continue;
        };

        let Some(&(target_pos, _)) = positions.get(&target_entity) else {
            continue;
        };

        let distance = my_pos.distance(target_pos);

        // Use class-specific preferred range, or pet-type preferred range for pets
        let stop_distance = if let Ok(pet) = pet_query.get(entity) {
            pet.pet_type.preferred_range()
        } else {
            combatant.class.preferred_range()
        };

        // If out of range, move towards target
        if distance > stop_distance {
            // Calculate direction to target (only in XZ plane, keep Y constant)
            let direction = Vec3::new(
                target_pos.x - my_pos.x,
                0.0, // Don't move vertically
                target_pos.z - my_pos.z,
            ).normalize_or_zero();

            if direction != Vec3::ZERO {
                // Calculate effective movement speed (base * aura modifiers)
                let mut movement_speed = combatant.base_movement_speed;
                if let Some(auras) = auras {
                    for aura in &auras.auras {
                        if aura.effect_type == AuraType::MovementSpeedSlow {
                            movement_speed *= aura.magnitude;
                        }
                    }
                }

                // Move towards target
                let move_distance = movement_speed * dt;
                transform.translation += direction * move_distance;

                // Clamp position to arena bounds
                transform.translation = clamp_to_arena(transform.translation);

                // Rotate to face target
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }
        }
    }
}
