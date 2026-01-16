//! Shadow Sight System
//!
//! Implements the Shadow Sight orb mechanic to break stealth stalemates.
//! After 90 seconds of combat, two orbs spawn that grant a buff allowing
//! the holder to see stealthed enemies (and be seen by enemies).

use bevy::prelude::*;
use crate::combat::log::{CombatLog, CombatLogEventType};
use super::components::*;
use super::PlayMatchEntity;

/// Time after gates open before Shadow Sight orbs spawn (seconds)
pub const SHADOW_SIGHT_SPAWN_TIME: f32 = 90.0;

/// Duration of the Shadow Sight buff (seconds)
pub const SHADOW_SIGHT_DURATION: f32 = 15.0;

/// Radius at which a combatant can pick up an orb
const ORB_PICKUP_RADIUS: f32 = 2.5;

/// Duration of the orb consumption animation (seconds)
const ORB_CONSUMPTION_DURATION: f32 = 0.4;

/// Spawn positions for Shadow Sight orbs (symmetric on Z-axis)
/// Positioned near the center but offset north/south for accessibility
const ORB_SPAWN_POSITIONS: [Vec3; 2] = [
    Vec3::new(0.0, 1.0, 15.0),   // North side
    Vec3::new(0.0, 1.0, -15.0),  // South side
];

/// System to track combat time and spawn Shadow Sight orbs after the threshold.
///
/// Runs every frame while gates are open, tracking elapsed combat time.
/// When combat time reaches SHADOW_SIGHT_SPAWN_TIME, spawns two orbs.
/// Works in both graphical and headless mode (visual components are optional).
pub fn track_shadow_sight_timer(
    time: Res<Time>,
    countdown: Res<MatchCountdown>,
    mut shadow_state: ResMut<ShadowSightState>,
    mut commands: Commands,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
    mut combat_log: ResMut<CombatLog>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't track time until gates open
    if !countdown.gates_opened {
        return;
    }

    // Don't spawn during victory celebration
    if celebration.is_some() {
        return;
    }

    shadow_state.combat_time += time.delta_secs();

    // Check if it's time to spawn orbs
    if !shadow_state.orbs_spawned && shadow_state.combat_time >= SHADOW_SIGHT_SPAWN_TIME {
        shadow_state.orbs_spawned = true;

        // Spawn both orbs (with or without visuals depending on mode)
        match (meshes, materials) {
            (Some(mut meshes), Some(mut materials)) => {
                // Graphical mode: spawn orbs with visuals
                for (i, position) in ORB_SPAWN_POSITIONS.iter().enumerate() {
                    spawn_shadow_sight_orb(&mut commands, &mut meshes, &mut materials, *position, i as u8);
                }
            }
            _ => {
                // Headless mode: spawn orbs without visuals
                for (i, position) in ORB_SPAWN_POSITIONS.iter().enumerate() {
                    commands.spawn((
                        Transform::from_translation(*position),
                        ShadowSightOrb { spawn_index: i as u8 },
                    ));
                }
            }
        }

        combat_log.log(
            CombatLogEventType::MatchEvent,
            "Shadow Sight orbs have spawned!".to_string(),
        );

        info!("Shadow Sight orbs spawned at {:?}", ORB_SPAWN_POSITIONS);
    }
}

/// Spawn a single Shadow Sight orb entity with visual representation.
/// Includes a core sphere and an outer glowing aura.
fn spawn_shadow_sight_orb(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    spawn_index: u8,
) {
    // Core orb - solid purple sphere
    let core_mesh = meshes.add(Sphere::new(0.5));
    let core_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.7, 0.2, 1.0, 1.0), // Bright purple
        emissive: LinearRgba::new(0.5, 0.1, 0.8, 1.0), // Purple glow
        ..default()
    });

    // Outer aura - larger, transparent, strongly emissive
    let aura_mesh = meshes.add(Sphere::new(1.0));
    let aura_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.6, 0.1, 0.9, 0.15), // Very transparent purple
        emissive: LinearRgba::new(0.8, 0.2, 1.2, 1.0), // Strong purple/magenta glow
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    // Spawn core orb with aura as child
    commands.spawn((
        Mesh3d(core_mesh),
        MeshMaterial3d(core_material),
        Transform::from_translation(position),
        ShadowSightOrb { spawn_index },
        PlayMatchEntity,
    )).with_child((
        Mesh3d(aura_mesh),
        MeshMaterial3d(aura_material),
        Transform::default(), // Centered on parent
    ));
}

/// System to detect combatants picking up Shadow Sight orbs.
///
/// When an alive combatant gets within ORB_PICKUP_RADIUS of an orb,
/// the orb is consumed and the combatant gains the Shadow Sight buff.
pub fn check_orb_pickups(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    combatants: Query<(Entity, &Combatant, &Transform)>,
    orbs: Query<(Entity, &ShadowSightOrb, &Transform), Without<ShadowSightOrbConsuming>>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't process pickups during victory celebration
    if celebration.is_some() {
        return;
    }

    for (orb_entity, _orb, orb_transform) in orbs.iter() {
        // Check if any alive combatant is within pickup radius
        for (combatant_entity, combatant, combatant_transform) in combatants.iter() {
            if !combatant.is_alive() {
                continue;
            }

            let distance = combatant_transform
                .translation
                .distance(orb_transform.translation);

            if distance <= ORB_PICKUP_RADIUS {
                // Combatant picks up the orb!

                // Mark orb as being consumed (will animate and despawn)
                commands.entity(orb_entity).insert(ShadowSightOrbConsuming {
                    collector: combatant_entity,
                    lifetime: ORB_CONSUMPTION_DURATION,
                    initial_lifetime: ORB_CONSUMPTION_DURATION,
                });

                // Apply Shadow Sight aura to the combatant
                commands.spawn(AuraPending {
                    target: combatant_entity,
                    aura: Aura {
                        effect_type: AuraType::ShadowSight,
                        duration: SHADOW_SIGHT_DURATION,
                        magnitude: 1.0, // Unused for this aura type
                        break_on_damage_threshold: 0.0, // Does not break on damage
                        accumulated_damage: 0.0,
                        tick_interval: 0.0,
                        time_until_next_tick: 0.0,
                        caster: None,
                        ability_name: "Shadow Sight".to_string(),
                        fear_direction: (0.0, 0.0),
                        fear_direction_timer: 0.0,
                    },
                });

                combat_log.log(
                    CombatLogEventType::Buff,
                    format!(
                        "Team {} {} picks up Shadow Sight!",
                        combatant.team,
                        combatant.class.name()
                    ),
                );

                info!(
                    "Team {} {} picked up Shadow Sight orb!",
                    combatant.team,
                    combatant.class.name()
                );

                // Only one combatant can pick up each orb
                break;
            }
        }
    }
}

/// Animate Shadow Sight orbs with bobbing, rotation, and pulsing.
pub fn animate_shadow_sight_orbs(
    time: Res<Time>,
    mut orbs: Query<&mut Transform, (With<ShadowSightOrb>, Without<ShadowSightOrbConsuming>)>,
) {
    let elapsed = time.elapsed_secs();

    // Pulsing scale (breathe effect)
    let pulse = (elapsed * 2.5).sin() * 0.12 + 1.0;

    // Vertical bobbing (gentle float up/down)
    let bob_offset = (elapsed * 1.5).sin() * 0.3;

    // Continuous rotation
    let rotation = Quat::from_rotation_y(elapsed * 1.2);

    for mut transform in orbs.iter_mut() {
        // Apply pulsing scale
        transform.scale = Vec3::splat(pulse);

        // Apply bobbing (adjust Y relative to base height of 1.0)
        transform.translation.y = 1.0 + bob_offset;

        // Apply rotation
        transform.rotation = rotation;
    }
}

/// Animate orbs being consumed - shrink and move toward the collector before despawning.
pub fn animate_orb_consumption(
    time: Res<Time>,
    mut commands: Commands,
    mut consuming_orbs: Query<(Entity, &mut Transform, &mut ShadowSightOrbConsuming), Without<Combatant>>,
    collectors: Query<&Transform, (With<Combatant>, Without<ShadowSightOrbConsuming>)>,
) {
    let delta = time.delta_secs();

    for (entity, mut orb_transform, mut consuming) in consuming_orbs.iter_mut() {
        consuming.lifetime -= delta;

        // Calculate animation progress (0 = just picked up, 1 = about to despawn)
        let progress = 1.0 - (consuming.lifetime / consuming.initial_lifetime).max(0.0);

        // Shrink the orb as it's consumed (ease out for snappy feel)
        let scale = (1.0 - progress).powi(2);
        orb_transform.scale = Vec3::splat(scale.max(0.01));

        // Move toward the collector if they still exist
        if let Ok(collector_transform) = collectors.get(consuming.collector) {
            let target = collector_transform.translation + Vec3::Y * 1.5; // Aim for chest height
            let direction = (target - orb_transform.translation).normalize_or_zero();
            let speed = 8.0 * progress + 2.0; // Accelerate as it gets closer
            orb_transform.translation += direction * speed * delta;
        }

        // Despawn when animation completes
        if consuming.lifetime <= 0.0 {
            commands.entity(entity).despawn_recursive();
        }
    }
}
