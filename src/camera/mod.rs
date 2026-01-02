//! Camera system
//!
//! Handles camera setup and controls for viewing the arena.
//! Supports multiple camera modes as specified in the design doc:
//! - Follow midpoint/center
//! - Zoom in/out
//! - Follow combatant
//! - Manual drag

use bevy::prelude::*;

/// Plugin for camera management
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraSettings>()
            .add_systems(Startup, setup_camera)
            .add_systems(Update, camera_controls);
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

/// Marker component for the main game camera
#[derive(Component)]
pub struct MainCamera;

fn setup_camera(mut commands: Commands) {
    // Spawn a 3D camera looking down at the arena at an angle
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 20.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        MainCamera,
    ));

    // Add ambient light
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 500.0,
    });

    // Add a directional light (sun-like)
    commands.spawn((
        DirectionalLight {
            color: Color::WHITE,
            illuminance: 10000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(10.0, 20.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn camera_controls(
    mut camera_query: Query<&mut Transform, With<MainCamera>>,
    settings: Res<CameraSettings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
) {
    let Ok(mut camera_transform) = camera_query.get_single_mut() else {
        return;
    };

    // Basic zoom with scroll wheel simulation via keyboard for now
    // TODO: Add proper mouse scroll zoom
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

    // Keep the camera looking at the center for now
    // TODO: Implement proper camera modes (follow center, follow combatant, manual)
    let _ = settings; // Suppress unused warning for now
}

