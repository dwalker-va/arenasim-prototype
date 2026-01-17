//! Play Match Scene - 3D Combat Arena
//!
//! This module handles the active match simulation where combatants battle each other.
//! Inspired by World of Warcraft's combat mechanics, it features:
//!
//! ## Combat System
//! - **Target Acquisition**: Combatants automatically find the nearest alive enemy
//! - **Movement**: Combatants move towards targets if out of range
//! - **Range Mechanics**: Melee attacks require being in melee range (2.5 units)
//! - **Auto-Attacks**: Each combatant attacks when in range, based on attack speed
//! - **Damage & Stats**: Tracks damage dealt/taken for each combatant
//! - **Win Conditions**: Match ends when all combatants of one team are eliminated
//!
//! ## Visual Representation
//! - 3D capsule meshes represent combatants, colored by class
//! - Health bars rendered above each combatant's head using 2D overlay
//! - Combatants rotate to face their targets
//! - Simple arena floor (60x60 plane)
//! - Isometric camera view
//!
//! ## Flow
//! 1. `setup_play_match`: Spawns arena, camera, lights, and combatants from `MatchConfig`
//! 2. Systems run each frame:
//!    - `update_play_match`: Handle ESC key to exit
//!    - `acquire_targets`: Find nearest enemy for each combatant
//!    - `move_to_target`: Move combatants towards targets if out of range
//!    - `combat_auto_attack`: Process attacks when in range, based on attack speed
//!    - `check_match_end`: Detect when match is over, transition to Results
//!    - `render_health_bars`: Draw 2D health bars over 3D combatants
//! 3. `cleanup_play_match`: Despawn all entities when exiting

// Submodules
pub mod abilities;
pub mod components;
pub mod camera;
pub mod projectiles;
pub mod rendering;
pub mod auras;
pub mod match_flow;
pub mod combat_ai;
pub mod combat_core;
pub mod shadow_sight;
pub mod systems;
pub mod utils;

// Re-exports
pub use abilities::*;
pub use components::*;
pub use camera::*;
pub use projectiles::*;
pub use rendering::*;
pub use auras::*;
pub use match_flow::*;
pub use combat_ai::*;
pub use combat_core::*;
pub use shadow_sight::*;
pub use utils::*;

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use super::match_config::{self, MatchConfig};
use super::GameState;
use crate::combat::log::{CombatLog, CombatLogEventType, PositionData, MatchMetadata, CombatantMetadata};

// ============================================================================
// Constants
// ============================================================================

/// Melee attack range in units. Combatants must be within this distance to auto-attack.
/// Similar to WoW's melee range of ~5 yards.
const MELEE_RANGE: f32 = 2.5;

/// Ranged wand attack range for caster classes (Mage, Priest).
/// Similar to WoW's wand range of ~30 yards.
const WAND_RANGE: f32 = 30.0;

/// Distance threshold for stopping movement (slightly less than melee range to avoid jitter)
const STOP_DISTANCE: f32 = 2.0;

/// Arena half-sizes for movement clamping (inside the visual walls)
/// The arena is 76x46 with wall centers at ±38/±23 and wall thickness 1.0.
/// We subtract 1.5 to account for wall thickness (0.5) + combatant buffer (1.0)
const ARENA_HALF_X: f32 = 36.5;  // X axis: wall center 38 - 1.5 buffer
const ARENA_HALF_Z: f32 = 21.5;  // Z axis: wall center 23 - 1.5 buffer

/// Floating combat text base height above combatants (in world space Y units)
/// Adjust this to control how high damage/healing numbers appear above characters
/// Should be high enough to avoid overlapping with status effect labels
const FCT_HEIGHT: f32 = 4.0;


// ============================================================================
// Helper Functions
// ============================================================================

/// Creates an octagonal floor mesh matching the arena wall layout
fn create_octagon_mesh(length: f32, width: f32, corner_cut: f32) -> Mesh {
    let half_length = length / 2.0;
    let half_width = width / 2.0;
    
    // Define the 8 vertices of the octagon (going counter-clockwise from top-right)
    // Y is up in 3D space, but for a floor plane we want XZ coordinates
    let vertices = vec![
        // Starting from north-east, going counter-clockwise
        [half_length - corner_cut, 0.0, half_width],           // 0: NE corner (right side of north edge)
        [-half_length + corner_cut, 0.0, half_width],          // 1: NW corner (left side of north edge)
        [-half_length, 0.0, half_width - corner_cut],          // 2: NW corner (top of west edge)
        [-half_length, 0.0, -half_width + corner_cut],         // 3: SW corner (bottom of west edge)
        [-half_length + corner_cut, 0.0, -half_width],         // 4: SW corner (left side of south edge)
        [half_length - corner_cut, 0.0, -half_width],          // 5: SE corner (right side of south edge)
        [half_length, 0.0, -half_width + corner_cut],          // 6: SE corner (bottom of east edge)
        [half_length, 0.0, half_width - corner_cut],           // 7: NE corner (top of east edge)
    ];
    
    // Create triangles by fanning from center point
    // Add center vertex
    let center = [0.0, 0.0, 0.0];
    let mut all_vertices = vertices.clone();
    all_vertices.push(center); // Index 8 is the center
    
    // Create triangle indices (center to each edge, going counter-clockwise)
    let indices = vec![
        8, 0, 1,  // North edge
        8, 1, 2,  // NW corner
        8, 2, 3,  // West edge
        8, 3, 4,  // SW corner
        8, 4, 5,  // South edge
        8, 5, 6,  // SE corner
        8, 6, 7,  // East edge
        8, 7, 0,  // NE corner
    ];
    
    // Create normals (all pointing up for a flat floor)
    let normals = vec![[0.0, 1.0, 0.0]; all_vertices.len()];
    
    // Create UVs (simple mapping)
    let uvs: Vec<[f32; 2]> = all_vertices.iter().map(|v| {
        [(v[0] / length) + 0.5, (v[2] / width) + 0.5]
    }).collect();
    
    Mesh::new(
        bevy::render::render_resource::PrimitiveTopology::TriangleList,
        bevy::render::render_asset::RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, all_vertices)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(bevy::render::mesh::Indices::U32(indices))
}

// ============================================================================
// Setup & Cleanup Systems
// ============================================================================

/// Setup system: Spawns the 3D arena, camera, lighting, and combatants.
/// 
/// This runs once when entering the PlayMatch state.
/// Reads the `MatchConfig` resource to determine team compositions.
pub fn setup_play_match(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut combat_log: ResMut<CombatLog>,
    config: Res<MatchConfig>,
) {
    info!("Setting up Play Match scene with config: {:?}", *config);

    // Clear combat log for new match
    combat_log.clear();
    combat_log.log(CombatLogEventType::MatchEvent, "Match started!".to_string());

    // Initialize combat panel view (for tabbed Combat Log / Timeline UI)
    commands.insert_resource(CombatPanelView::default());

    // Initialize spell icons resources (for ability timeline)
    commands.insert_resource(SpellIcons::default());
    commands.insert_resource(SpellIconHandles::default());

    // Spawn 3D camera with isometric-ish view
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 40.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
        ArenaCamera,
        PlayMatchEntity,
    ));

    // Add directional light (sun-like) - warm golden sunlight
    commands.spawn((
        DirectionalLight {
            illuminance: 25000.0,
            color: Color::srgb(1.0, 0.95, 0.85), // Warm golden sunlight
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        PlayMatchEntity,
    ));

    // Add ambient light for overall scene brightness - warm atmospheric glow
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.9, 0.85, 0.7), // Warm peachy ambient light
        brightness: 400.0,
    });
    
    // Initialize simulation speed control
    commands.insert_resource(SimulationSpeed { multiplier: 1.0 });
    
    // Initialize camera controller
    commands.insert_resource(CameraController::default());
    
    // Initialize match countdown (10 seconds before gates open)
    commands.insert_resource(MatchCountdown::default());

    // Initialize Shadow Sight state (for stealth stalemate breaking)
    commands.insert_resource(ShadowSightState::default());

    // Initialize random number generator (non-deterministic for graphical mode)
    commands.insert_resource(GameRng::default());

    // Spawn arena floor - octagonal shape matching the wall boundary
    // Warm sandy/dirt battleground
    let arena_length = 76.0;
    let arena_width = 46.0;
    let corner_cut = 10.0;
    
    // Create custom octagonal mesh
    let octagon_mesh = create_octagon_mesh(arena_length, arena_width, corner_cut);
    
    commands.spawn((
        Mesh3d(meshes.add(octagon_mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.79, 0.66, 0.46), // Warm sandy tan (#c9a876)
            perceptual_roughness: 0.95, // Matte dirt/sand texture
            cull_mode: None, // Render both sides
            ..default()
        })),
        PlayMatchEntity,
    ));

    // Spawn rectangular arena walls with chamfered corners (simplified stadium shape)
    let wall_height = 4.0;
    let wall_thickness = 1.0;
    
    // Warm weathered stone material for walls
    let wall_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.54, 0.45, 0.33), // Weathered stone brown (#8b7355)
        perceptual_roughness: 0.9,
        ..default()
    });
    
    // Arena dimensions: elongated octagon
    let corner_cut = 10.0; // How much to cut off each corner
    let half_length = arena_length / 2.0; // 38.0
    let half_width = arena_width / 2.0;   // 23.0
    
    // Calculate wall dimensions
    let long_wall_length = arena_length - corner_cut * 2.0; // North/South walls
    let short_wall_length = arena_width - corner_cut * 2.0; // East/West walls
    let corner_wall_length = corner_cut * 1.414; // Diagonal length (45° angle)
    
    // North wall (positive Z) - main long side
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(long_wall_length, wall_height, wall_thickness))),
        MeshMaterial3d(wall_material.clone()),
        Transform::from_xyz(0.0, wall_height / 2.0, half_width),
        PlayMatchEntity,
    ));
    
    // South wall (negative Z) - main long side
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(long_wall_length, wall_height, wall_thickness))),
        MeshMaterial3d(wall_material.clone()),
        Transform::from_xyz(0.0, wall_height / 2.0, -half_width),
        PlayMatchEntity,
    ));
    
    // East wall (positive X) - short side
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(wall_thickness, wall_height, short_wall_length))),
        MeshMaterial3d(wall_material.clone()),
        Transform::from_xyz(half_length, wall_height / 2.0, 0.0),
        PlayMatchEntity,
    ));
    
    // West wall (negative X) - short side
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(wall_thickness, wall_height, short_wall_length))),
        MeshMaterial3d(wall_material.clone()),
        Transform::from_xyz(-half_length, wall_height / 2.0, 0.0),
        PlayMatchEntity,
    ));
    
    // Add angled corner pieces to connect the walls (45-degree angles)
    // Each corner wall connects the end of one straight wall to the end of another
    let corner_offset_x = half_length - corner_cut / 2.0;
    let corner_offset_z = half_width - corner_cut / 2.0;
    
    // Northeast corner (connects North wall to East wall)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(corner_wall_length, wall_height, wall_thickness))),
        MeshMaterial3d(wall_material.clone()),
        Transform::from_xyz(corner_offset_x, wall_height / 2.0, corner_offset_z)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI / 4.0)),
        PlayMatchEntity,
    ));
    
    // Southeast corner (connects South wall to East wall)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(corner_wall_length, wall_height, wall_thickness))),
        MeshMaterial3d(wall_material.clone()),
        Transform::from_xyz(corner_offset_x, wall_height / 2.0, -corner_offset_z)
            .with_rotation(Quat::from_rotation_y(-std::f32::consts::PI / 4.0)),
        PlayMatchEntity,
    ));
    
    // Northwest corner (connects North wall to West wall)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(corner_wall_length, wall_height, wall_thickness))),
        MeshMaterial3d(wall_material.clone()),
        Transform::from_xyz(-corner_offset_x, wall_height / 2.0, corner_offset_z)
            .with_rotation(Quat::from_rotation_y(-std::f32::consts::PI / 4.0)),
        PlayMatchEntity,
    ));
    
    // Southwest corner (connects South wall to West wall)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(corner_wall_length, wall_height, wall_thickness))),
        MeshMaterial3d(wall_material.clone()),
        Transform::from_xyz(-corner_offset_x, wall_height / 2.0, -corner_offset_z)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::PI / 4.0)),
        PlayMatchEntity,
    ));

    // Count class occurrences per team to apply darkening to duplicates
    use std::collections::HashMap;
    let mut team1_class_counts: HashMap<match_config::CharacterClass, usize> = HashMap::new();
    let mut team2_class_counts: HashMap<match_config::CharacterClass, usize> = HashMap::new();

    // Spawn Team 1 combatants (left side of arena, in starting pen)
    // Teams start further back (-35/+35) and will move forward when gates open
    let team1_spawn_x = -35.0;
    for (i, character_opt) in config.team1.iter().enumerate() {
        if let Some(character) = character_opt {
            let count = *team1_class_counts.get(character).unwrap_or(&0);
            *team1_class_counts.entry(*character).or_insert(0) += 1;

            // Register combatant with combat log for timeline display
            combat_log.register_combatant(combatant_id(1, *character));

            spawn_combatant(
                &mut commands,
                &mut meshes,
                &mut materials,
                1,
                *character,
                Vec3::new(team1_spawn_x, 1.0, (i as f32 - 1.0) * 3.0),
                count,
            );
        }
    }

    // Spawn Team 2 combatants (right side of arena, in starting pen)
    let team2_spawn_x = 35.0;
    for (i, character_opt) in config.team2.iter().enumerate() {
        if let Some(character) = character_opt {
            let count = *team2_class_counts.get(character).unwrap_or(&0);
            *team2_class_counts.entry(*character).or_insert(0) += 1;

            // Register combatant with combat log for timeline display
            combat_log.register_combatant(combatant_id(2, *character));

            spawn_combatant(
                &mut commands,
                &mut meshes,
                &mut materials,
                2,
                *character,
                Vec3::new(team2_spawn_x, 1.0, (i as f32 - 1.0) * 3.0),
                count,
            );
        }
    }
    
    // Spawn starting gate bars for both teams
    spawn_gate_bars(&mut commands, &mut meshes, &mut materials, team1_spawn_x, team2_spawn_x);
}

/// Spawn visual gate bars that lower when countdown ends
fn spawn_gate_bars(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    team1_x: f32,
    team2_x: f32,
) {
    let gate_height = 6.0;
    let bar_width = 0.5;
    let bar_depth = 0.5;
    let num_bars = 7; // Number of vertical bars per gate
    let spacing = 2.5; // Space between bars
    
    // Dark metal material for the bars
    let bar_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.2, 0.2), // Dark gray/metal
        metallic: 0.8,
        perceptual_roughness: 0.3,
        ..default()
    });
    
    // Team 1 gate (left side)
    for i in 0..num_bars {
        let z_offset = (i as f32 - (num_bars as f32 / 2.0)) * spacing;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(bar_width, gate_height, bar_depth))),
            MeshMaterial3d(bar_material.clone()),
            Transform::from_xyz(team1_x + 3.0, gate_height / 2.0, z_offset),
            GateBar {
                team: 1,
                initial_height: gate_height,
            },
            PlayMatchEntity,
        ));
    }
    
    // Team 2 gate (right side)
    for i in 0..num_bars {
        let z_offset = (i as f32 - (num_bars as f32 / 2.0)) * spacing;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(bar_width, gate_height, bar_depth))),
            MeshMaterial3d(bar_material.clone()),
            Transform::from_xyz(team2_x - 3.0, gate_height / 2.0, z_offset),
            GateBar {
                team: 2,
                initial_height: gate_height,
            },
            PlayMatchEntity,
        ));
    }
}

/// Helper function to spawn a single combatant entity.
/// 
/// Creates a capsule mesh colored by class, with darker shades for duplicates.
/// The `duplicate_index` parameter determines how much to darken (0 = base color, 1+ = darkened).
fn spawn_combatant(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    team: u8,
    class: match_config::CharacterClass,
    position: Vec3,
    duplicate_index: usize,
) {
    // Get vibrant class colors for 3D visibility
    let base_color = match class {
        match_config::CharacterClass::Warrior => Color::srgb(0.9, 0.6, 0.3), // Orange/brown
        match_config::CharacterClass::Mage => Color::srgb(0.3, 0.6, 1.0),    // Bright blue
        match_config::CharacterClass::Rogue => Color::srgb(1.0, 0.9, 0.2),   // Bright yellow
        match_config::CharacterClass::Priest => Color::srgb(0.95, 0.95, 0.95), // White
        match_config::CharacterClass::Warlock => Color::srgb(0.58, 0.41, 0.93), // Purple
    };
    
    // Apply darkening for duplicate classes (0.65 multiplier per duplicate)
    let darken_factor = 0.65f32.powi(duplicate_index as i32);
    let combatant_color = Color::srgb(
        base_color.to_srgba().red * darken_factor,
        base_color.to_srgba().green * darken_factor,
        base_color.to_srgba().blue * darken_factor,
    );

    // Create combatant mesh (capsule represents the body)
    let mesh = meshes.add(Capsule3d::new(0.5, 1.5));
    let material = materials.add(StandardMaterial {
        base_color: combatant_color,
        perceptual_roughness: 0.5, // More reflective for better color visibility
        metallic: 0.2, // Slight metallic sheen for color pop
        // Enable alpha mode for stealth transparency
        alpha_mode: bevy::prelude::AlphaMode::Blend,
        ..default()
    });

    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(position),
        Combatant::new(team, class),
        FloatingTextState {
            next_pattern_index: 0,
        },
        PlayMatchEntity,
    ));
}

/// Handle camera input for mode switching, zoom, rotation, and drag

/// Cleanup system: Despawns all Play Match entities when exiting the state.
pub fn cleanup_play_match(
    mut commands: Commands,
    query: Query<Entity, With<PlayMatchEntity>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
    
    // Remove resources
    commands.remove_resource::<AmbientLight>();
    commands.remove_resource::<SimulationSpeed>();
    commands.remove_resource::<MatchCountdown>();
    commands.remove_resource::<ShadowSightState>();
    // Remove optional resources (may not exist if match didn't finish)
    commands.remove_resource::<VictoryCelebration>();
}

// ============================================================================
// Update & Input Systems
// ============================================================================

/// Countdown system: Manage pre-combat countdown and gate opening.
/// 
/// During countdown (10 seconds):
/// - Mana is restored to 100% every second (encourages pre-buffing)
/// - Combatants can cast buffs but cannot move or attack
/// - Countdown timer ticks down
/// 
/// When countdown reaches 0:
/// - Gates open (sets gates_opened flag)
/// - Combat begins normally

/// Render time control UI panel in the top-right corner.
/// 
/// Shows current speed and clickable buttons for speed control.
/// Handle player input during the match.
/// Currently only handles ESC key to return to main menu.
pub fn update_play_match(
    keybindings: Res<crate::keybindings::Keybindings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    use crate::keybindings::GameAction;
    
    if keybindings.action_just_pressed(GameAction::Back, &keyboard) {
        next_state.set(GameState::MainMenu);
    }
}

// ============================================================================
// Combat Systems (see submodules: combat_ai, combat_core, auras, projectiles)
// ============================================================================

