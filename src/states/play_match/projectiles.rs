//! Projectile Systems
//!
//! Handles spell projectiles that travel from caster to target.

use bevy::prelude::*;
use bevy_egui::egui;
use crate::combat::log::CombatLog;
use super::match_config;
use super::components::*;
use super::abilities::AbilityType;
use super::ability_config::AbilityDefinitions;
use super::constants::CRIT_DAMAGE_MULTIPLIER;
use super::utils::{combatant_id, get_next_fct_offset};

/// Spawn visual meshes for newly created projectiles.
/// Creates a glowing sphere that travels through the air.
/// Note: Projectiles already have a Transform (added in process_casting for headless compatibility).
pub fn spawn_projectile_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_projectiles: Query<(Entity, &Projectile), (Added<Projectile>, Without<Mesh3d>)>,
) {
    for (projectile_entity, projectile) in new_projectiles.iter() {
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

        // Add visual mesh to the projectile entity (Transform already exists from process_casting)
        commands.entity(projectile_entity).insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
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
    mut game_rng: ResMut<GameRng>,
    abilities: Res<AbilityDefinitions>,
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
    // Format: (projectile_entity, caster_entity, target_entity, ability, caster_team, caster_class, caster_pos, target_pos, ability_damage, ability_healing, is_crit)
    let mut hits_to_process: Vec<(Entity, Entity, Entity, AbilityType, u8, match_config::CharacterClass, Vec3, Vec3, f32, f32, bool)> = Vec::new();
    
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
            // Get caster data (position, combatant stats, auras) in a single query
            let Ok((caster_transform, caster_combatant, caster_auras)) = combatants.get(projectile.caster) else {
                // Caster no longer exists, despawn projectile
                commands.entity(projectile_entity).despawn_recursive();
                continue;
            };

            let caster_pos = caster_transform.translation;
            let target_world_pos = target_transform.translation;

            let def = abilities.get_unchecked(&projectile.ability);
            let mut ability_damage = caster_combatant.calculate_ability_damage_config(def, &mut game_rng);
            let ability_healing = caster_combatant.calculate_ability_healing_config(def, &mut game_rng);

            // Roll crit at impact time using caster's live crit_chance
            let is_crit = super::combat_core::roll_crit(caster_combatant.crit_chance, &mut game_rng);
            if is_crit {
                ability_damage *= CRIT_DAMAGE_MULTIPLIER;
            }

            // Apply Divine Shield outgoing damage penalty (50%) at impact time
            let ds_penalty = super::combat_core::get_divine_shield_damage_penalty(caster_auras.as_deref());
            ability_damage = (ability_damage * ds_penalty).max(0.0);

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
                is_crit,
            ));
        }
    }
    
    // Process all queued hits
    for (projectile_entity, caster_entity, target_entity, ability, caster_team, caster_class, caster_pos, target_pos, ability_damage, _ability_healing, is_crit) in hits_to_process {
        let def = abilities.get_unchecked(&ability);
        let text_position = target_pos + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
        let _ability_range = caster_pos.distance(target_pos);
        
        // Apply damage
        if def.is_damage() {
            // Use pre-calculated damage (already includes stat scaling)
            let damage = ability_damage;

            // Get target info and apply damage
            let (actual_damage, absorbed, target_team, target_class, is_killing_blow, is_first_death) = {
                let Ok((_, mut target, mut target_auras)) = combatants.get_mut(target_entity) else {
                    commands.entity(projectile_entity).despawn_recursive();
                    continue;
                };

                // Apply damage with absorb shield consideration
                let (actual_damage, absorbed) = super::combat_core::apply_damage_with_absorb(
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
                let is_first_death = is_killing_blow && !target.is_dead;
                if is_first_death {
                    target.is_dead = true;
                }
                (actual_damage, absorbed, target.team, target.class, is_killing_blow, is_first_death)
            }; // target borrow dropped here

            // Update caster damage dealt (include absorbed damage - caster dealt it)
            {
                let Ok((_, mut caster, _)) = combatants.get_mut(caster_entity) else {
                    commands.entity(projectile_entity).despawn_recursive();
                    continue;
                };
                caster.damage_dealt += actual_damage + absorbed;
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
                    is_crit,
                },
                PlayMatchEntity,
            ));

            // Spawn light blue floating combat text for absorbed damage
            if absorbed > 0.0 {
                let (absorb_offset_x, absorb_offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                    get_next_fct_offset(&mut fct_state)
                } else {
                    (0.0, 0.0)
                };
                commands.spawn((
                    FloatingCombatText {
                        world_position: text_position + Vec3::new(absorb_offset_x, absorb_offset_y, 0.0),
                        text: format!("{:.0} absorbed", absorbed),
                        color: egui::Color32::from_rgb(100, 180, 255), // Light blue
                        lifetime: 1.5,
                        vertical_offset: absorb_offset_y,
                        is_crit: false,
                    },
                    PlayMatchEntity,
                ));
            }

            // Log the damage with structured data
            let verb = if is_crit { "CRITS" } else { "hits" };
            let message = if absorbed > 0.0 {
                format!(
                    "Team {} {}'s {} {} Team {} {} for {:.0} damage ({:.0} absorbed)",
                    caster_team,
                    caster_class.name(),
                    def.name,
                    verb,
                    target_team,
                    target_class.name(),
                    actual_damage,
                    absorbed
                )
            } else {
                format!(
                    "Team {} {}'s {} {} Team {} {} for {:.0} damage",
                    caster_team,
                    caster_class.name(),
                    def.name,
                    verb,
                    target_team,
                    target_class.name(),
                    actual_damage
                )
            };
            combat_log.log_damage(
                combatant_id(caster_team, caster_class),
                combatant_id(target_team, target_class),
                def.name.to_string(),
                actual_damage + absorbed, // Total damage dealt (including absorbed)
                is_killing_blow,
                is_crit,
                message,
            );

            // Log death with killer tracking (only on first death to prevent duplicates)
            if is_first_death {
                // Cancel any in-progress cast or channel so dead combatants can't finish spells
                commands.entity(target_entity).remove::<CastingState>();
                commands.entity(target_entity).remove::<ChannelingState>();

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
            
            // Apply aura if ability has one (skip if target was killed — don't CC dead combatants)
            if !is_killing_blow {
            if let Some(aura) = def.applies_aura.as_ref() {
                // Convert spell school to Option (None for Physical, since physical = not magic-dispellable)
                let aura_spell_school = match def.spell_school {
                    super::abilities::SpellSchool::Physical | super::abilities::SpellSchool::None => None,
                    school => Some(school),
                };
                commands.spawn(AuraPending {
                    target: target_entity,
                    aura: Aura {
                        effect_type: aura.aura_type,
                        duration: aura.duration,
                        magnitude: aura.magnitude,
                        break_on_damage_threshold: aura.break_on_damage,
                        accumulated_damage: 0.0,
                        tick_interval: if aura.aura_type == AuraType::DamageOverTime { aura.tick_interval } else { 0.0 },
                        time_until_next_tick: if aura.aura_type == AuraType::DamageOverTime { aura.tick_interval } else { 0.0 },
                        caster: Some(caster_entity),
                        ability_name: def.name.to_string(),
                        fear_direction: (0.0, 0.0),
                        fear_direction_timer: 0.0,
                        spell_school: aura_spell_school,
                    },
                });
            }
            }
        }

        // Despawn the projectile
        commands.entity(projectile_entity).despawn_recursive();
    }
}

