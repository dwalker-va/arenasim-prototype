use bevy::prelude::*;
use bevy::color::LinearRgba;
use crate::states::play_match::components::*;

// ==============================================================================
// Trap Visual Helpers
// ==============================================================================

/// Base RGB color for a trap type. Frost = cyan, Freezing = deep blue.
pub(crate) fn trap_type_rgb(trap_type: TrapType) -> (f32, f32, f32) {
    match trap_type {
        TrapType::Frost => (0.3, 0.8, 1.0),
        TrapType::Freezing => (0.3, 0.55, 1.0),
    }
}

/// Emissive glow for a trap type.
pub(crate) fn trap_type_emissive(trap_type: TrapType) -> LinearRgba {
    match trap_type {
        TrapType::Frost => LinearRgba::new(0.4, 1.2, 2.0, 1.0),
        TrapType::Freezing => LinearRgba::new(0.6, 1.2, 2.8, 1.0),
    }
}

// ==============================================================================
// Trap Ground Circle Visual (spawned on Trap entity via Added<Trap>)
// ==============================================================================

/// Spawn flat cylinder mesh on newly created traps to visualize their position.
/// Color depends on trap type: Frost = cyan, Freezing = ice-white.
pub fn spawn_trap_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_traps: Query<(Entity, &Trap), (Added<Trap>, Without<Mesh3d>)>,
) {
    for (trap_entity, trap) in new_traps.iter() {
        let mesh = meshes.add(Cylinder::new(2.0, 0.05));

        let (r, g, b) = trap_type_rgb(trap.trap_type);
        let base_color = Color::srgba(r, g, b, 0.15); // Dim while arming
        let emissive = trap_type_emissive(trap.trap_type);

        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        commands.entity(trap_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
        ));
    }
}

/// Update trap ground circles: dim pulse while arming, bright shimmer when armed.
pub fn update_trap_visuals(
    time: Res<Time>,
    traps: Query<(&Trap, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let t = time.elapsed_secs();

    for (trap, material_handle) in traps.iter() {
        let Some(material) = materials.get_mut(&material_handle.0) else {
            continue;
        };

        let (r, g, b) = trap_type_rgb(trap.trap_type);
        let emissive_base = trap_type_emissive(trap.trap_type);

        if trap.arm_timer > 0.0 {
            // Arming: low alpha with slow sine pulse
            let pulse = 0.1 + 0.05 * (t * 2.0).sin();
            material.base_color = Color::srgba(r, g, b, pulse);
            // Dim emissive while arming
            material.emissive = LinearRgba::new(
                emissive_base.red * 0.3,
                emissive_base.green * 0.3,
                emissive_base.blue * 0.3,
                1.0,
            );
        } else {
            // Armed: full brightness with subtle shimmer
            let shimmer = 0.35 + 0.05 * (t * 4.0).sin();
            material.base_color = Color::srgba(r, g, b, shimmer);
            material.emissive = emissive_base;
        }
    }
}

// ==============================================================================
// Trap Burst Visual (expanding sphere on trigger)
// ==============================================================================

/// Spawn visual mesh for trap burst effects.
pub fn spawn_trap_burst_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_bursts: Query<(Entity, &TrapBurst), (Added<TrapBurst>, Without<Mesh3d>)>,
) {
    for (burst_entity, burst) in new_bursts.iter() {
        let mesh = meshes.add(Sphere::new(0.6));

        let (r, g, b) = trap_type_rgb(burst.trap_type);
        let base_color = Color::srgba(r, g, b, 0.6);
        // Burst uses brighter emissive than ground circle
        let emissive = match burst.trap_type {
            TrapType::Frost => LinearRgba::new(0.6, 1.5, 2.5, 1.0),
            TrapType::Freezing => LinearRgba::new(0.8, 1.5, 3.5, 1.0),
        };

        let material = materials.add(StandardMaterial {
            base_color,
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        commands.entity(burst_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
        ));
    }
}

/// Update and cleanup trap bursts: expand scale and fade, despawn when expired.
pub fn update_and_cleanup_trap_bursts(
    mut commands: Commands,
    time: Res<Time>,
    mut bursts: Query<(Entity, &mut TrapBurst, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();

    for (entity, mut burst, mut transform, material_handle) in bursts.iter_mut() {
        burst.lifetime -= dt;

        if burst.lifetime <= 0.0 {
            commands.entity(entity).despawn();
            continue;
        }

        // Progress: 1.0 (just spawned) → 0.0 (expired)
        let progress = (burst.lifetime / burst.initial_lifetime).max(0.0);

        // Scale up: 1.0 → 4.0
        let scale = 1.0 + (1.0 - progress) * 3.0;
        transform.scale = Vec3::splat(scale);

        // Fade out
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let alpha = 0.6 * progress;
            let (r, g, b) = trap_type_rgb(burst.trap_type);
            material.base_color = Color::srgba(r, g, b, alpha);
        }
    }
}

// ==============================================================================
// Trap Launch Arc Visual (in-flight sphere while trap travels to landing position)
// ==============================================================================

/// Spawn glowing sphere mesh on newly created trap launch projectiles.
pub fn spawn_trap_launch_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_projectiles: Query<(Entity, &TrapLaunchProjectile), (Added<TrapLaunchProjectile>, Without<Mesh3d>)>,
) {
    for (entity, proj) in new_projectiles.iter() {
        let mesh = meshes.add(Sphere::new(0.3));

        let (r, g, b) = trap_type_rgb(proj.trap_type);
        let emissive = trap_type_emissive(proj.trap_type);

        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(r, g, b, 0.8),
            emissive,
            alpha_mode: AlphaMode::Add,
            ..default()
        });

        commands.entity(entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
        ));
    }
}

// ==============================================================================
// Ice Block Visual (Freezing Trap cuboid)
// ==============================================================================

/// Spawn translucent ice cuboid around Freezing Trap targets.
pub fn spawn_ice_block_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_blocks: Query<(Entity, &IceBlockVisual), (Added<IceBlockVisual>, Without<Mesh3d>)>,
    transforms: Query<&Transform>,
) {
    for (block_entity, block) in new_blocks.iter() {
        let Ok(target_transform) = transforms.get(block.target) else {
            continue;
        };

        let mesh = meshes.add(Cuboid::new(1.5, 2.3, 1.5));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.3, 0.6, 1.0, 0.45),
            emissive: LinearRgba::new(0.5, 1.0, 2.8, 1.0),
            alpha_mode: AlphaMode::Blend,
            ..default()
        });

        commands.entity(block_entity).try_insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(target_transform.translation),
        ));
    }
}

/// Update ice block positions to follow their frozen targets.
pub fn update_ice_blocks(
    mut ice_blocks: Query<(&IceBlockVisual, &mut Transform), Without<Combatant>>,
    combatants: Query<&Transform, With<Combatant>>,
) {
    for (block, mut block_transform) in ice_blocks.iter_mut() {
        if let Ok(target_transform) = combatants.get(block.target) {
            block_transform.translation = target_transform.translation;
        }
    }
}

/// Cleanup ice blocks when the Incapacitate aura breaks or target dies.
pub fn cleanup_ice_blocks(
    mut commands: Commands,
    time: Res<Time>,
    mut ice_blocks: Query<(Entity, &mut IceBlockVisual)>,
    combatants: Query<(&Combatant, Option<&ActiveAuras>)>,
) {
    let dt = time.delta_secs();
    for (block_entity, mut block) in ice_blocks.iter_mut() {
        // Grace period: skip cleanup check to let apply_pending_auras process the aura
        if block.grace_timer > 0.0 {
            block.grace_timer -= dt;
            continue;
        }
        let should_despawn = match combatants.get(block.target) {
            Ok((combatant, auras)) => {
                // Despawn if target died
                if !combatant.is_alive() {
                    true
                } else {
                    // Despawn if target no longer has Incapacitate aura
                    auras.map_or(true, |a| {
                        !a.auras.iter().any(|aura| aura.effect_type == AuraType::Incapacitate)
                    })
                }
            }
            Err(_) => true, // Target entity gone
        };

        if should_despawn {
            commands.entity(block_entity).despawn();
        }
    }
}

