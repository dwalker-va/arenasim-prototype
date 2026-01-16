//! Projectile Systems
//!
//! Handles spell projectiles that travel from caster to target.

use bevy::prelude::*;
use bevy_egui::egui;
use crate::combat::log::CombatLog;
use super::match_config;
use super::components::*;
use super::abilities::AbilityType;
use super::get_next_fct_offset;
use super::combat_core::combatant_id;

/// Spawn visual meshes for newly created projectiles.
/// Creates a glowing sphere that travels through the air.
pub fn spawn_projectile_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_projectiles: Query<(Entity, &Projectile), (Added<Projectile>, Without<Mesh3d>)>,
    combatants: Query<&Transform, With<Combatant>>,
) {
    for (projectile_entity, projectile) in new_projectiles.iter() {
        // Get caster position to spawn projectile at that location
        let Ok(caster_transform) = combatants.get(projectile.caster) else {
            continue;
        };
        
        let caster_pos = caster_transform.translation;

        // Create a small sphere mesh for the projectile
        let mesh = meshes.add(Sphere::new(0.3));

        // Color based on spell school/ability type
        let (base_color, emissive) = match projectile.ability {
            AbilityType::Shadowbolt => (
                Color::srgb(0.6, 0.3, 0.8),           // Purple
                LinearRgba::rgb(0.8, 0.4, 1.2),       // Purple glow
            ),
            AbilityType::Frostbolt => (
                Color::srgb(0.4, 0.7, 1.0),           // Ice blue
                LinearRgba::rgb(0.6, 0.9, 1.5),       // Ice glow
            ),
            _ => (
                Color::srgb(1.0, 0.8, 0.3),           // Default: golden/arcane
                LinearRgba::rgb(1.2, 1.0, 0.5),       // Golden glow
            ),
        };

        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            ..default()
        });
        
        // Add visual mesh to the projectile entity
        commands.entity(projectile_entity).insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(caster_pos + Vec3::new(0.0, 1.5, 0.0)), // Start at chest height
        ));
    }
}

/// Move projectiles towards their targets.
/// Projectiles travel in a straight line at their defined speed.
pub fn move_projectiles(
    time: Res<Time>,
    mut projectiles: Query<(&Projectile, &mut Transform)>,
    targets: Query<&Transform, (With<Combatant>, Without<Projectile>)>,
) {
    let dt = time.delta_secs();
    
    for (projectile, mut projectile_transform) in projectiles.iter_mut() {
        // Get target position
        let Ok(target_transform) = targets.get(projectile.target) else {
            continue; // Target no longer exists
        };
        
        let target_pos = target_transform.translation + Vec3::new(0.0, 1.0, 0.0); // Aim at center mass
        let current_pos = projectile_transform.translation;
        
        // Calculate direction to target
        let direction = (target_pos - current_pos).normalize_or_zero();
        
        if direction != Vec3::ZERO {
            // Move towards target
            let move_distance = projectile.speed * dt;
            projectile_transform.translation += direction * move_distance;
            
            // Rotate to face direction of travel
            let target_rotation = Quat::from_rotation_arc(Vec3::Z, direction);
            projectile_transform.rotation = target_rotation;
        }
    }
}

/// Check if projectiles have reached their targets and apply effects.
/// When a projectile gets close enough to its target, it "hits" and applies damage/healing/auras.
pub fn process_projectile_hits(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    projectiles: Query<(Entity, &Projectile, &Transform)>,
    mut combatants: Query<(&Transform, &mut Combatant, Option<&mut ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't apply projectile damage during victory celebration
    if celebration.is_some() {
        return;
    }
    
    const HIT_DISTANCE: f32 = 0.5; // Projectile hits when within 0.5 units of target
    
    // Collect hits to process (to avoid borrow checker issues)
    // Format: (projectile_entity, caster_entity, target_entity, ability, caster_team, caster_class, caster_pos, target_pos, ability_damage, ability_healing)
    let mut hits_to_process: Vec<(Entity, Entity, Entity, AbilityType, u8, match_config::CharacterClass, Vec3, Vec3, f32, f32)> = Vec::new();
    
    for (projectile_entity, projectile, projectile_transform) in projectiles.iter() {
        // Get target position (immutable borrow)
        let Ok((target_transform, target, _)) = combatants.get(projectile.target) else {
            // Target no longer exists, despawn projectile
            commands.entity(projectile_entity).despawn_recursive();
            continue;
        };

        if !target.is_alive() {
            // Target already dead, despawn projectile
            commands.entity(projectile_entity).despawn_recursive();
            continue;
        }

        let target_pos = target_transform.translation + Vec3::new(0.0, 1.0, 0.0); // Center mass
        let projectile_pos = projectile_transform.translation;
        let distance = projectile_pos.distance(target_pos);

        // Check if projectile has reached target
        if distance <= HIT_DISTANCE {
            // Get caster position (immutable borrow)
            let Ok((caster_transform, _, _)) = combatants.get(projectile.caster) else {
                // Caster no longer exists, despawn projectile
                commands.entity(projectile_entity).despawn_recursive();
                continue;
            };

            let caster_pos = caster_transform.translation;
            let target_world_pos = target_transform.translation;

            // Get caster's combatant to calculate damage/healing with stats
            let Ok((_, caster_combatant, _)) = combatants.get(projectile.caster) else {
                commands.entity(projectile_entity).despawn_recursive();
                continue;
            };
            
            let def = projectile.ability.definition();
            let ability_damage = caster_combatant.calculate_ability_damage(&def);
            let ability_healing = caster_combatant.calculate_ability_healing(&def);
            
            // Queue this hit for processing
            hits_to_process.push((
                projectile_entity,
                projectile.caster,
                projectile.target,
                projectile.ability,
                projectile.caster_team,
                projectile.caster_class,
                caster_pos,
                target_world_pos,
                ability_damage,
                ability_healing,
            ));
        }
    }
    
    // Process all queued hits
    for (projectile_entity, caster_entity, target_entity, ability, caster_team, caster_class, caster_pos, target_pos, ability_damage, _ability_healing) in hits_to_process {
        let def = ability.definition();
        let text_position = target_pos + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
        let ability_range = caster_pos.distance(target_pos);
        
        // Apply damage
        if def.is_damage() {
            // Use pre-calculated damage (already includes stat scaling)
            let damage = ability_damage;

            // Get target info and apply damage
            let (actual_damage, target_team, target_class, is_killing_blow) = {
                let Ok((_, mut target, mut target_auras)) = combatants.get_mut(target_entity) else {
                    commands.entity(projectile_entity).despawn_recursive();
                    continue;
                };

                // Apply damage with absorb shield consideration
                let (actual_damage, _absorbed) = super::combat_core::apply_damage_with_absorb(
                    damage,
                    &mut target,
                    target_auras.as_deref_mut(),
                );

                // Warriors generate Rage from taking damage (only on actual health damage)
                if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15;
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }

                // Track damage for aura breaking
                commands.entity(target_entity).insert(DamageTakenThisFrame {
                    amount: actual_damage,
                });

                let is_killing_blow = !target.is_alive();
                (actual_damage, target.team, target.class, is_killing_blow)
            }; // target borrow dropped here

            // Update caster damage dealt
            {
                let Ok((_, mut caster, _)) = combatants.get_mut(caster_entity) else {
                    commands.entity(projectile_entity).despawn_recursive();
                    continue;
                };
                caster.damage_dealt += actual_damage;
            } // caster borrow dropped here
            
            // Spawn yellow floating combat text for ability damage
            // Get deterministic offset based on pattern state
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: format!("{:.0}", actual_damage),
                    color: egui::Color32::from_rgb(255, 255, 0), // Yellow
                    lifetime: 1.5,
                    vertical_offset: offset_y,
                },
                PlayMatchEntity,
            ));
            
            // Log the damage with structured data
            let message = format!(
                "Team {} {}'s {} hits Team {} {} for {:.0} damage",
                caster_team,
                caster_class.name(),
                def.name,
                target_team,
                target_class.name(),
                actual_damage
            );
            combat_log.log_damage(
                combatant_id(caster_team, caster_class),
                combatant_id(target_team, target_class),
                def.name.to_string(),
                actual_damage,
                is_killing_blow,
                message,
            );

            // Log death with killer tracking
            if is_killing_blow {
                let death_message = format!(
                    "Team {} {} has been eliminated",
                    target_team,
                    target_class.name()
                );
                combat_log.log_death(
                    combatant_id(target_team, target_class),
                    Some(combatant_id(caster_team, caster_class)),
                    death_message,
                );
            }
            
            // Apply aura if ability has one
            if let Some((aura_type, duration, magnitude, break_threshold)) = def.applies_aura {
                commands.spawn(AuraPending {
                    target: target_entity,
                    aura: Aura {
                        effect_type: aura_type,
                        duration,
                        magnitude,
                        break_on_damage_threshold: break_threshold,
                        accumulated_damage: 0.0,
                        tick_interval: if aura_type == AuraType::DamageOverTime { 3.0 } else { 0.0 },
                        time_until_next_tick: if aura_type == AuraType::DamageOverTime { 3.0 } else { 0.0 },
                        caster: Some(caster_entity),
                        ability_name: def.name.to_string(),
                        fear_direction: (0.0, 0.0),
                        fear_direction_timer: 0.0,
                    },
                });
            }
        }
        
        // Despawn the projectile
        commands.entity(projectile_entity).despawn_recursive();
    }
}

