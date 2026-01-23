//! Camera Control Systems
//!
//! Handles camera modes, input, and positioning for the match view.

use bevy::prelude::*;
use bevy::time::Real;
use bevy_egui::{egui, EguiContexts};
use super::components::{CameraController, CameraMode, ArenaCamera, Combatant};

/// Handle camera input for mode switching, zoom, rotation, and drag
pub fn handle_camera_input(
    mut camera_controller: ResMut<CameraController>,
    keybindings: Res<crate::keybindings::Keybindings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut mouse_wheel: EventReader<bevy::input::mouse::MouseWheel>,
    mut cursor_moved: EventReader<bevy::window::CursorMoved>,
    time: Res<Time<Real>>,
    combatants: Query<Entity, With<Combatant>>,
    mut contexts: EguiContexts,
) {
    use crate::keybindings::GameAction;

    // Use real (wall-clock) time so camera works even when simulation is paused
    let dt = time.delta_secs();

    // Check if egui wants pointer input (hovering over UI)
    // Use try_ctx_mut to gracefully handle window close
    let egui_wants_pointer = contexts.try_ctx_mut()
        .map(|ctx| ctx.wants_pointer_input())
        .unwrap_or(false);

    // Keyboard zoom controls
    if keybindings.action_pressed(GameAction::CameraZoomIn, &keyboard) {
        let zoom_speed = 30.0 * dt;
        camera_controller.zoom_distance = (camera_controller.zoom_distance - zoom_speed).clamp(20.0, 150.0);
    }
    if keybindings.action_pressed(GameAction::CameraZoomOut, &keyboard) {
        let zoom_speed = 30.0 * dt;
        camera_controller.zoom_distance = (camera_controller.zoom_distance + zoom_speed).clamp(20.0, 150.0);
    }

    // WASD camera panning - moves the target point
    camera_controller.keyboard_movement = Vec3::ZERO;
    let move_speed = 15.0 * dt;

    if keybindings.action_pressed(GameAction::CameraMoveForward, &keyboard) {
        camera_controller.keyboard_movement.z -= move_speed;
    }
    if keybindings.action_pressed(GameAction::CameraMoveBackward, &keyboard) {
        camera_controller.keyboard_movement.z += move_speed;
    }
    if keybindings.action_pressed(GameAction::CameraMoveLeft, &keyboard) {
        camera_controller.keyboard_movement.x -= move_speed;
    }
    if keybindings.action_pressed(GameAction::CameraMoveRight, &keyboard) {
        camera_controller.keyboard_movement.x += move_speed;
    }

    // Cycle camera modes
    if keybindings.action_just_pressed(GameAction::CycleCameraMode, &keyboard) {
        camera_controller.mode = match camera_controller.mode {
            CameraMode::FollowCenter => {
                // Find first alive combatant to follow
                if let Some(entity) = combatants.iter().next() {
                    CameraMode::FollowCombatant(entity)
                } else {
                    CameraMode::FollowCenter
                }
            }
            CameraMode::FollowCombatant(current_entity) => {
                // Cycle to next combatant
                let mut found_current = false;
                let mut next_entity = None;

                for entity in combatants.iter() {
                    if found_current {
                        next_entity = Some(entity);
                        break;
                    }
                    if entity == current_entity {
                        found_current = true;
                    }
                }

                // If we found a next entity, use it. Otherwise, go to manual or back to center
                if let Some(entity) = next_entity {
                    CameraMode::FollowCombatant(entity)
                } else {
                    CameraMode::Manual
                }
            }
            CameraMode::Manual => CameraMode::FollowCenter,
        };
    }

    // Reset camera
    if keybindings.action_just_pressed(GameAction::ResetCamera, &keyboard) {
        camera_controller.mode = CameraMode::FollowCenter;
        camera_controller.zoom_distance = 60.0;
        camera_controller.pitch = 38.7f32.to_radians();
        camera_controller.yaw = 0.0;
    }

    // Handle mouse wheel for zoom (only if not over UI)
    if !egui_wants_pointer {
        for event in mouse_wheel.read() {
            let zoom_delta = event.y * 3.0; // Zoom speed
            camera_controller.zoom_distance = (camera_controller.zoom_distance - zoom_delta).clamp(20.0, 150.0);
        }
    } else {
        // Drain events if egui wants pointer
        mouse_wheel.clear();
    }

    // Handle mouse drag for rotation (left mouse button, only if not over UI)
    if mouse_button.just_pressed(MouseButton::Left) && !egui_wants_pointer {
        camera_controller.is_dragging = true;

        // When starting manual mode, we need to preserve the current target
        // We'll update manual_target in the update_camera_position system
    }

    if mouse_button.just_released(MouseButton::Left) {
        camera_controller.is_dragging = false;
        camera_controller.last_mouse_pos = None;
    }

    if camera_controller.is_dragging {
        for event in cursor_moved.read() {
            if let Some(last_pos) = camera_controller.last_mouse_pos {
                let delta = event.position - last_pos;

                // Update yaw and pitch based on drag
                camera_controller.yaw -= delta.x * 0.005; // Horizontal rotation
                camera_controller.pitch = (camera_controller.pitch - delta.y * 0.005).clamp(0.1, 1.5); // Vertical rotation, clamped
            }
            camera_controller.last_mouse_pos = Some(event.position);
        }
    } else {
        // Update last mouse pos even when not dragging, so first drag frame isn't a huge jump
        for event in cursor_moved.read() {
            camera_controller.last_mouse_pos = Some(event.position);
        }
    }
}

/// Update camera position and rotation based on controller state
pub fn update_camera_position(
    mut camera_controller: ResMut<CameraController>,
    mut camera_query: Query<&mut Transform, With<ArenaCamera>>,
    combatants: Query<(Entity, &Transform, &Combatant), Without<ArenaCamera>>,
) {
    let Ok(mut camera_transform) = camera_query.get_single_mut() else {
        return;
    };
    
    // If user just started dragging OR using keyboard movement, switch to manual mode and preserve current target
    let needs_manual_switch = (camera_controller.is_dragging || camera_controller.keyboard_movement != Vec3::ZERO) 
        && camera_controller.mode != CameraMode::Manual;
    
    if needs_manual_switch {
        // Calculate current target before switching to manual
        let current_target = match camera_controller.mode {
            CameraMode::FollowCenter => {
                let alive_combatants: Vec<Vec3> = combatants
                    .iter()
                    .filter(|(_, _, c)| c.is_alive())
                    .map(|(_, t, _)| t.translation)
                    .collect();
                if alive_combatants.is_empty() {
                    Vec3::ZERO
                } else {
                    let sum: Vec3 = alive_combatants.iter().sum();
                    sum / alive_combatants.len() as f32
                }
            }
            CameraMode::FollowCombatant(target_entity) => {
                combatants
                    .iter()
                    .find(|(e, _, _)| *e == target_entity)
                    .map(|(_, t, _)| t.translation)
                    .unwrap_or(Vec3::ZERO)
            }
            CameraMode::Manual => camera_controller.manual_target,
        };
        
        camera_controller.manual_target = current_target;
        camera_controller.mode = CameraMode::Manual;
    }
    
    // Apply keyboard movement to manual target, rotated by camera yaw
    // so that WASD moves relative to camera orientation
    let keyboard_movement = camera_controller.keyboard_movement;
    if keyboard_movement != Vec3::ZERO {
        let yaw = camera_controller.yaw;

        // Calculate camera-relative directions (projected to XZ plane)
        // Forward is toward the target (negative of camera offset direction)
        let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
        let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());

        // keyboard_movement.z is forward/back input, keyboard_movement.x is left/right input
        // Negative Z = forward (W key), positive X = right (D key)
        let rotated_movement = forward * (-keyboard_movement.z) + right * keyboard_movement.x;

        camera_controller.manual_target += rotated_movement;
    }
    
    // Determine the target look-at point based on camera mode
    let target_point = match camera_controller.mode {
        CameraMode::FollowCenter => {
            // Calculate center of all alive combatants
            let alive_combatants: Vec<Vec3> = combatants
                .iter()
                .filter(|(_, _, c)| c.is_alive())
                .map(|(_, t, _)| t.translation)
                .collect();
            
            if alive_combatants.is_empty() {
                Vec3::ZERO
            } else {
                let sum: Vec3 = alive_combatants.iter().sum();
                sum / alive_combatants.len() as f32
            }
        }
        CameraMode::FollowCombatant(target_entity) => {
            // Follow specific combatant
            combatants
                .iter()
                .find(|(e, _, _)| *e == target_entity)
                .map(|(_, t, _)| t.translation)
                .unwrap_or(Vec3::ZERO)
        }
        CameraMode::Manual => {
            // Use manual target (preserved when entering manual mode)
            camera_controller.manual_target
        }
    };
    
    // Calculate camera position based on spherical coordinates
    let x = target_point.x + camera_controller.zoom_distance * camera_controller.pitch.sin() * camera_controller.yaw.sin();
    let y = target_point.y + camera_controller.zoom_distance * camera_controller.pitch.cos();
    let z = target_point.z + camera_controller.zoom_distance * camera_controller.pitch.sin() * camera_controller.yaw.cos();
    
    camera_transform.translation = Vec3::new(x, y, z);
    camera_transform.look_at(target_point, Vec3::Y);
}

/// Render camera controls help overlay
pub fn render_camera_controls(
    mut contexts: EguiContexts,
    camera_controller: Res<CameraController>,
    keybindings: Res<crate::keybindings::Keybindings>,
) {
    use crate::keybindings::GameAction;

    // Use try_ctx_mut to gracefully handle window close
    let Some(ctx) = contexts.try_ctx_mut() else { return; };

    // Position in bottom-right corner (to avoid overlapping with timeline panel on left)
    let panel_width = 260.0;
    egui::Window::new("Camera Controls")
        .fixed_pos(egui::pos2(ctx.screen_rect().width() - panel_width - 10.0, ctx.screen_rect().height() - 160.0))
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style())
            .fill(egui::Color32::from_black_alpha(150)) // Semi-transparent
            .stroke(egui::Stroke::NONE)) // Remove border
        .show(ctx, |ui| {
            ui.set_width(250.0);
            
            // Current mode
            let mode_text = match camera_controller.mode {
                CameraMode::FollowCenter => "Center",
                CameraMode::FollowCombatant(_) => "Follow Combatant",
                CameraMode::Manual => "Manual",
            };
            
            ui.label(
                egui::RichText::new(format!("Mode: {}", mode_text))
                    .size(12.0)
                    .color(egui::Color32::from_rgb(100, 200, 255))
                    .strong()
            );
            
            ui.add_space(5.0);
            
            // Controls - dynamically show actual keybindings
            ui.label(
                egui::RichText::new(format!(
                    "{} - Cycle camera mode",
                    keybindings.binding_display(GameAction::CycleCameraMode)
                ))
                .size(11.0)
                .color(egui::Color32::from_rgb(200, 200, 200))
            );
            ui.label(
                egui::RichText::new(format!(
                    "{} - Reset to center",
                    keybindings.binding_display(GameAction::ResetCamera)
                ))
                .size(11.0)
                .color(egui::Color32::from_rgb(200, 200, 200))
            );
            ui.label(
                egui::RichText::new("Mouse Wheel - Zoom")
                    .size(11.0)
                    .color(egui::Color32::from_rgb(200, 200, 200))
            );
            ui.label(
                egui::RichText::new("Left Click Drag - Rotate/Pitch")
                    .size(11.0)
                    .color(egui::Color32::from_rgb(200, 200, 200))
            );
        });
}

