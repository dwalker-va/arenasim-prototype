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

/// Handle Back key to return to previous state/menu.
/// 
/// Note: ConfigureMatch has its own Back handler to close modals first.
fn handle_escape_key(
    keybindings: Res<crate::keybindings::Keybindings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    current_state: Res<State<GameState>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    use crate::keybindings::GameAction;
    
    if keybindings.action_just_pressed(GameAction::Back, &keyboard) {
        match current_state.get() {
            GameState::MainMenu => {
                // ESC in main menu does nothing (or could open quit confirmation)
            }
            GameState::Options => {
                next_state.set(GameState::MainMenu);
            }
            GameState::Keybindings => {
                next_state.set(GameState::Options);
            }
            GameState::ConfigureMatch => {
                // ConfigureMatch has its own ESC handler - skip here
            }
            GameState::ViewCombatant => {
                // ViewCombatant has its own ESC handler - skip here
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
/// - Zoom in/out: Via keybindings (default: +/- or numpad +/-)
/// - Pan camera: Via keybindings (default: WASD)
fn camera_controls(
    mut camera_query: Query<&mut Transform, With<MainCamera>>,
    keybindings: Res<crate::keybindings::Keybindings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    use crate::keybindings::GameAction;
    
    let Ok(mut camera_transform) = camera_query.get_single_mut() else {
        return;
    };

    // Zoom controls (move camera forward/backward along view direction)
    let zoom_speed = 10.0 * time.delta_secs();
    if keybindings.action_pressed(GameAction::CameraZoomIn, &keyboard) {
        let direction = camera_transform.forward();
        camera_transform.translation += direction * zoom_speed;
    }
    if keybindings.action_pressed(GameAction::CameraZoomOut, &keyboard) {
        let direction = camera_transform.forward();
        camera_transform.translation -= direction * zoom_speed;
    }

    // Pan controls (move camera in world space)
    let move_speed = 15.0 * time.delta_secs();
    if keybindings.action_pressed(GameAction::CameraMoveForward, &keyboard) {
        camera_transform.translation.z -= move_speed;
    }
    if keybindings.action_pressed(GameAction::CameraMoveBackward, &keyboard) {
        camera_transform.translation.z += move_speed;
    }
    if keybindings.action_pressed(GameAction::CameraMoveLeft, &keyboard) {
        camera_transform.translation.x -= move_speed;
    }
    if keybindings.action_pressed(GameAction::CameraMoveRight, &keyboard) {
        camera_transform.translation.x += move_speed;
    }
}
