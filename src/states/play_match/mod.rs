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

/// Arena size (80x80 plane centered at origin, includes starting areas)
const ARENA_HALF_SIZE: f32 = 40.0;

/// Floating combat text horizontal spread (multiplied by -0.5 to +0.5 range)
/// Adjust this to control how far left/right numbers can appear from their spawn point
const FCT_HORIZONTAL_SPREAD: f32 = 1.2; // Default: 0.8 (range: -0.4 to +0.4)

/// Floating combat text vertical spread (0.0 to this value)
/// Adjust this to control the vertical stagger of numbers
const FCT_VERTICAL_SPREAD: f32 = 0.8; // Default: 0.5 (range: 0.0 to 0.5)


// ============================================================================
// Helper Functions
// ============================================================================

/// Helper function to get next floating combat text offset and update pattern state
/// Returns (x_offset, y_offset) based on deterministic alternating pattern
fn get_next_fct_offset(state: &mut FloatingTextState) -> (f32, f32) {
    let (x_offset, y_offset) = match state.next_pattern_index {
        0 => (0.0, 0.0),  // Center
        1 => (FCT_HORIZONTAL_SPREAD * 0.4, FCT_VERTICAL_SPREAD * 0.3),  // Right side, slight up
        2 => (FCT_HORIZONTAL_SPREAD * -0.4, FCT_VERTICAL_SPREAD * 0.6), // Left side, more up
        _ => (0.0, 0.0),  // Fallback to center
    };
    
    // Cycle to next pattern: 0 -> 1 -> 2 -> 0
    state.next_pattern_index = (state.next_pattern_index + 1) % 3;
    
    (x_offset, y_offset)
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

    // Spawn 3D camera with isometric-ish view
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 40.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
        ArenaCamera,
        PlayMatchEntity,
    ));

    // Add directional light (sun-like)
    commands.spawn((
        DirectionalLight {
            illuminance: 20000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        PlayMatchEntity,
    ));

    // Add ambient light for overall scene brightness
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.3, 0.3, 0.4),
        brightness: 300.0,
    });
    
    // Initialize simulation speed control
    commands.insert_resource(SimulationSpeed { multiplier: 1.0 });
    
    // Initialize camera controller
    commands.insert_resource(CameraController::default());
    
    // Initialize match countdown (10 seconds before gates open)
    commands.insert_resource(MatchCountdown::default());

    // Spawn arena floor - 80x80 unit plane (includes starting areas)
    let floor_size = 80.0;
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(floor_size, floor_size))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.25, 0.3),
            perceptual_roughness: 0.9,
            ..default()
        })),
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

