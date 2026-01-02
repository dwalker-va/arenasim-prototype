//! Camera system
//!
//! Handles camera setup and controls for viewing the arena.
//! Supports multiple camera modes as specified in the design doc:
//! - Follow midpoint/center
//! - Zoom in/out
//! - Follow combatant
//! - Manual drag

use bevy::prelude::*;

use crate::states::GameState;

/// Plugin for camera management
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraSettings>()
            .add_systems(Update, handle_escape_key)
            .add_systems(
                Update,
                camera_controls.run_if(in_state(GameState::PlayMatch)),
            );
    }
}

/// Camera control mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraMode {
    /// Follow the midpoint between all combatants
    #[default]
    FollowCenter,
    /// Follow a specific combatant
    FollowCombatant(Entity),
    /// Manual camera control via drag
    Manual,
}

/// Global camera settings
#[derive(Resource)]
pub struct CameraSettings {
    /// Current camera mode
    pub mode: CameraMode,
    /// Current zoom level (distance from target)
    pub zoom: f32,
    /// Minimum zoom distance
    pub zoom_min: f32,
    /// Maximum zoom distance
    pub zoom_max: f32,
    /// Camera movement smoothing factor
    pub smoothing: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            mode: CameraMode::FollowCenter,
            zoom: 20.0,
            zoom_min: 5.0,
            zoom_max: 50.0,
            smoothing: 5.0,
        }
    }
}

/// Marker component for the main 3D game camera (used during PlayMatch)
#[derive(Component)]
pub struct MainCamera;

/// Handle ESC key to return to previous state/menu
/// Note: ConfigureMatch has its own ESC handler to close modals first
fn handle_escape_key(
    keyboard: Res<ButtonInput<KeyCode>>,
    current_state: Res<State<GameState>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        match current_state.get() {
            GameState::MainMenu => {
                // ESC in main menu does nothing (or could open quit confirmation)
            }
            GameState::Options => {
                next_state.set(GameState::MainMenu);
            }
            GameState::ConfigureMatch => {
                // ConfigureMatch has its own ESC handler - skip here
            }
            GameState::PlayMatch => {
                // During a match, ESC could pause or show a menu
                // For now, just return to main menu
                next_state.set(GameState::MainMenu);
            }
            GameState::Results => {
                next_state.set(GameState::MainMenu);
            }
        }
    }
}

/// Camera controls for the 3D arena view during PlayMatch
fn camera_controls(
    mut camera_query: Query<&mut Transform, With<MainCamera>>,
    _settings: Res<CameraSettings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    let Ok(mut camera_transform) = camera_query.get_single_mut() else {
        return;
    };

    // Basic zoom with scroll wheel simulation via keyboard for now
    let zoom_speed = 10.0 * time.delta_secs();
    if keyboard.pressed(KeyCode::Equal) || keyboard.pressed(KeyCode::NumpadAdd) {
        let direction = camera_transform.forward();
        camera_transform.translation += direction * zoom_speed;
    }
    if keyboard.pressed(KeyCode::Minus) || keyboard.pressed(KeyCode::NumpadSubtract) {
        let direction = camera_transform.forward();
        camera_transform.translation -= direction * zoom_speed;
    }

    // Basic WASD camera movement in manual mode
    let move_speed = 15.0 * time.delta_secs();
    if keyboard.pressed(KeyCode::KeyW) {
        camera_transform.translation.z -= move_speed;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        camera_transform.translation.z += move_speed;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        camera_transform.translation.x -= move_speed;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        camera_transform.translation.x += move_speed;
    }
}
