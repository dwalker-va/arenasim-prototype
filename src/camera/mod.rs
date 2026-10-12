//! Camera system
//!
//! Handles camera setup and basic keyboard controls for viewing the 3D arena during matches.
//!
//! **Current Features:**
//! - Fixed isometric camera position
//! - Keyboard-based zoom (+/- or numpad +/-)
//! - Keyboard-based panning (WASD)
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
        app.add_systems(
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

    let Ok(mut camera_transform) = camera_query.single_mut() else {
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
