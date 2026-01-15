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
fn spawn_shadow_sight_orb(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    spawn_index: u8,
) {
    // Create a glowing purple sphere for the orb
    let mesh = meshes.add(Sphere::new(0.6));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.8, 0.2, 1.0, 0.9), // Purple glow
        emissive: LinearRgba::new(0.6, 0.1, 0.8, 1.0),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(position),
        ShadowSightOrb { spawn_index },
        PlayMatchEntity,
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
    orbs: Query<(Entity, &ShadowSightOrb, &Transform)>,
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

                // Despawn the orb
                commands.entity(orb_entity).despawn_recursive();

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

/// Animate Shadow Sight orbs with a pulsing effect.
pub fn animate_shadow_sight_orbs(
    time: Res<Time>,
    mut orbs: Query<&mut Transform, With<ShadowSightOrb>>,
) {
    let pulse = (time.elapsed_secs() * 3.0).sin() * 0.15 + 1.0;

    for mut transform in orbs.iter_mut() {
        transform.scale = Vec3::splat(pulse);
    }
}
