//! Camera system
//!
//! Handles camera setup and basic keyboard controls for viewing the 3D arena during matches.
//! 
//! **Current Features:**
//! - Fixed isometric camera position
//! - Keyboard-based zoom (+/- or numpad +/-)
//! - Keyboard-based panning (WASD)
//! - ESC key handling for navigation
//!
//! **Future Enhancements:**
//! - Follow midpoint/center of combat
//! - Follow specific combatant
//! - Mouse-based drag controls
//! - Smooth camera transitions

use bevy::prelude::*;

use crate::states::GameState;

/// Plugin for camera management
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, handle_escape_key)
            .add_systems(
                Update,
                camera_controls.run_if(in_state(GameState::PlayMatch)),
            );
    }
}

/// Marker component for the main 3D game camera (used during PlayMatch).
/// 
/// The camera is spawned in `setup_play_match` and despawned in `cleanup_play_match`.
#[derive(Component)]
pub struct MainCamera;

/// Handle ESC key to return to previous state/menu.
/// 
/// Note: ConfigureMatch has its own ESC handler to close modals first.
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
                // During a match, ESC returns to main menu
                // Future: Could show pause menu instead
                next_state.set(GameState::MainMenu);
            }
            GameState::Results => {
                next_state.set(GameState::MainMenu);
            }
        }
    }
}

/// Camera controls for the 3D arena view during PlayMatch.
/// 
/// **Controls:**
/// - `+` / `Numpad +`: Zoom in
/// - `-` / `Numpad -`: Zoom out
/// - `WASD`: Pan camera (move viewpoint)
fn camera_controls(
    mut camera_query: Query<&mut Transform, With<MainCamera>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    let Ok(mut camera_transform) = camera_query.get_single_mut() else {
        return;
    };

    // Zoom controls (move camera forward/backward along view direction)
    let zoom_speed = 10.0 * time.delta_secs();
    if keyboard.pressed(KeyCode::Equal) || keyboard.pressed(KeyCode::NumpadAdd) {
        let direction = camera_transform.forward();
        camera_transform.translation += direction * zoom_speed;
    }
    if keyboard.pressed(KeyCode::Minus) || keyboard.pressed(KeyCode::NumpadSubtract) {
        let direction = camera_transform.forward();
        camera_transform.translation -= direction * zoom_speed;
    }

    // Pan controls (move camera in world space)
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
