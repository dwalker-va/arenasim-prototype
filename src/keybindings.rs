//! Keybinding system for remappable controls
//!
//! Allows players to customize game controls and save their preferences.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// All possible actions that can be bound to keys
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GameAction {
    // Navigation
    Back,
    Confirm,
    
    // Camera
    CycleCameraMode,
    ResetCamera,
    CameraMoveForward,
    CameraMoveBackward,
    CameraMoveLeft,
    CameraMoveRight,
    CameraZoomIn,
    CameraZoomOut,
    
    // Simulation
    PausePlay,
    SpeedSlow,
    SpeedNormal,
    SpeedFast,
    SpeedVeryFast,
}

impl GameAction {
    pub fn description(&self) -> &'static str {
        match self {
            GameAction::Back => "Back / Cancel",
            GameAction::Confirm => "Confirm / Select",
            GameAction::CycleCameraMode => "Cycle Camera Mode",
            GameAction::ResetCamera => "Reset Camera",
            GameAction::CameraMoveForward => "Camera Forward",
            GameAction::CameraMoveBackward => "Camera Backward",
            GameAction::CameraMoveLeft => "Camera Left",
            GameAction::CameraMoveRight => "Camera Right",
            GameAction::CameraZoomIn => "Camera Zoom In",
            GameAction::CameraZoomOut => "Camera Zoom Out",
            GameAction::PausePlay => "Pause / Play",
            GameAction::SpeedSlow => "Speed: 0.5x",
            GameAction::SpeedNormal => "Speed: 1x",
            GameAction::SpeedFast => "Speed: 2x",
            GameAction::SpeedVeryFast => "Speed: 3x",
        }
    }
    
    pub fn category(&self) -> &'static str {
        match self {
            GameAction::Back | GameAction::Confirm => "Navigation",
            GameAction::CycleCameraMode | GameAction::ResetCamera 
            | GameAction::CameraMoveForward | GameAction::CameraMoveBackward
            | GameAction::CameraMoveLeft | GameAction::CameraMoveRight
            | GameAction::CameraZoomIn | GameAction::CameraZoomOut => "Camera",
            GameAction::PausePlay | GameAction::SpeedSlow 
            | GameAction::SpeedNormal | GameAction::SpeedFast 
            | GameAction::SpeedVeryFast => "Simulation",
        }
    }
    
    pub fn all() -> Vec<GameAction> {
        vec![
            GameAction::Back,
            GameAction::Confirm,
            GameAction::CycleCameraMode,
            GameAction::ResetCamera,
            GameAction::CameraMoveForward,
            GameAction::CameraMoveBackward,
            GameAction::CameraMoveLeft,
            GameAction::CameraMoveRight,
            GameAction::CameraZoomIn,
            GameAction::CameraZoomOut,
            GameAction::PausePlay,
            GameAction::SpeedSlow,
            GameAction::SpeedNormal,
            GameAction::SpeedFast,
            GameAction::SpeedVeryFast,
        ]
    }
}

/// Serializable wrapper for KeyCode (stores as string)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct SerializableKeyCode(String);

impl From<KeyCode> for SerializableKeyCode {
    fn from(key: KeyCode) -> Self {
        Self(format!("{:?}", key))
    }
}

impl From<SerializableKeyCode> for KeyCode {
    fn from(sk: SerializableKeyCode) -> Self {
        // Parse the debug string back to KeyCode
        // This is a simple implementation - in production you'd want more robust parsing
        match sk.0.as_str() {
            "Escape" => KeyCode::Escape,
            "Enter" => KeyCode::Enter,
            "Space" => KeyCode::Space,
            "Tab" => KeyCode::Tab,
            "Backspace" => KeyCode::Backspace,
            "KeyA" => KeyCode::KeyA,
            "KeyB" => KeyCode::KeyB,
            "KeyC" => KeyCode::KeyC,
            "KeyD" => KeyCode::KeyD,
            "KeyE" => KeyCode::KeyE,
            "KeyF" => KeyCode::KeyF,
            "KeyG" => KeyCode::KeyG,
            "KeyH" => KeyCode::KeyH,
            "KeyI" => KeyCode::KeyI,
            "KeyJ" => KeyCode::KeyJ,
            "KeyK" => KeyCode::KeyK,
            "KeyL" => KeyCode::KeyL,
            "KeyM" => KeyCode::KeyM,
            "KeyN" => KeyCode::KeyN,
            "KeyO" => KeyCode::KeyO,
            "KeyP" => KeyCode::KeyP,
            "KeyQ" => KeyCode::KeyQ,
            "KeyR" => KeyCode::KeyR,
            "KeyS" => KeyCode::KeyS,
            "KeyT" => KeyCode::KeyT,
            "KeyU" => KeyCode::KeyU,
            "KeyV" => KeyCode::KeyV,
            "KeyW" => KeyCode::KeyW,
            "KeyX" => KeyCode::KeyX,
            "KeyY" => KeyCode::KeyY,
            "KeyZ" => KeyCode::KeyZ,
            "Digit1" => KeyCode::Digit1,
            "Digit2" => KeyCode::Digit2,
            "Digit3" => KeyCode::Digit3,
            "Digit4" => KeyCode::Digit4,
            "Digit5" => KeyCode::Digit5,
            "Digit6" => KeyCode::Digit6,
            "Digit7" => KeyCode::Digit7,
            "Digit8" => KeyCode::Digit8,
            "Digit9" => KeyCode::Digit9,
            "Digit0" => KeyCode::Digit0,
            "F1" => KeyCode::F1,
            "F2" => KeyCode::F2,
            "F3" => KeyCode::F3,
            "F4" => KeyCode::F4,
            "F5" => KeyCode::F5,
            "F6" => KeyCode::F6,
            "F7" => KeyCode::F7,
            "F8" => KeyCode::F8,
            "F9" => KeyCode::F9,
            "F10" => KeyCode::F10,
            "F11" => KeyCode::F11,
            "F12" => KeyCode::F12,
            "Minus" => KeyCode::Minus,
            "Equal" => KeyCode::Equal,
            "NumpadAdd" => KeyCode::NumpadAdd,
            "NumpadSubtract" => KeyCode::NumpadSubtract,
            "NumpadMultiply" => KeyCode::NumpadMultiply,
            "NumpadDivide" => KeyCode::NumpadDivide,
            "ArrowUp" => KeyCode::ArrowUp,
            "ArrowDown" => KeyCode::ArrowDown,
            "ArrowLeft" => KeyCode::ArrowLeft,
            "ArrowRight" => KeyCode::ArrowRight,
            _ => KeyCode::Escape, // Default fallback
        }
    }
}

/// Key binding with primary and optional secondary key
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyBinding {
    #[serde(with = "keycode_serde")]
    pub primary: KeyCode,
    #[serde(with = "option_keycode_serde")]
    pub secondary: Option<KeyCode>,
}

mod keycode_serde {
    use super::*;
    use serde::{Deserializer, Serializer};
    
    pub fn serialize<S>(key: &KeyCode, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let sk: SerializableKeyCode = (*key).into();
        sk.serialize(serializer)
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<KeyCode, D::Error>
    where
        D: Deserializer<'de>,
    {
        let sk = SerializableKeyCode::deserialize(deserializer)?;
        Ok(sk.into())
    }
}

mod option_keycode_serde {
    use super::*;
    use serde::{Deserializer, Serializer};
    
    pub fn serialize<S>(key: &Option<KeyCode>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match key {
            Some(k) => {
                let sk: SerializableKeyCode = (*k).into();
                serializer.serialize_some(&sk)
            }
            None => serializer.serialize_none(),
        }
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<KeyCode>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt_sk: Option<SerializableKeyCode> = Option::deserialize(deserializer)?;
        Ok(opt_sk.map(|sk| sk.into()))
    }
}

impl KeyBinding {
    pub fn new(primary: KeyCode) -> Self {
        Self {
            primary,
            secondary: None,
        }
    }
    
    pub fn with_secondary(primary: KeyCode, secondary: KeyCode) -> Self {
        Self {
            primary,
            secondary: Some(secondary),
        }
    }
    
    pub fn matches(&self, key: KeyCode) -> bool {
        self.primary == key || self.secondary == Some(key)
    }
}

/// Complete keybindings configuration
#[derive(Debug, Clone, Resource, Serialize, Deserialize)]
pub struct Keybindings {
    bindings: HashMap<GameAction, KeyBinding>,
}

impl Default for Keybindings {
    fn default() -> Self {
        Self::create_defaults()
    }
}

impl Keybindings {
    /// Create default keybindings
    pub fn create_defaults() -> Self {
        let mut bindings = HashMap::new();
        
        // Navigation
        bindings.insert(GameAction::Back, KeyBinding::new(KeyCode::Escape));
        bindings.insert(GameAction::Confirm, KeyBinding::new(KeyCode::Enter));
        
        // Camera
        bindings.insert(GameAction::CycleCameraMode, KeyBinding::new(KeyCode::Tab));
        bindings.insert(GameAction::ResetCamera, KeyBinding::new(KeyCode::KeyC));
        bindings.insert(GameAction::CameraMoveForward, KeyBinding::new(KeyCode::KeyW));
        bindings.insert(GameAction::CameraMoveBackward, KeyBinding::new(KeyCode::KeyS));
        bindings.insert(GameAction::CameraMoveLeft, KeyBinding::new(KeyCode::KeyA));
        bindings.insert(GameAction::CameraMoveRight, KeyBinding::new(KeyCode::KeyD));
        bindings.insert(GameAction::CameraZoomIn, KeyBinding::with_secondary(
            KeyCode::Equal,
            KeyCode::NumpadAdd
        ));
        bindings.insert(GameAction::CameraZoomOut, KeyBinding::with_secondary(
            KeyCode::Minus,
            KeyCode::NumpadSubtract
        ));
        
        // Simulation
        bindings.insert(GameAction::PausePlay, KeyBinding::new(KeyCode::Space));
        bindings.insert(GameAction::SpeedSlow, KeyBinding::new(KeyCode::Digit1));
        bindings.insert(GameAction::SpeedNormal, KeyBinding::new(KeyCode::Digit2));
        bindings.insert(GameAction::SpeedFast, KeyBinding::new(KeyCode::Digit3));
        bindings.insert(GameAction::SpeedVeryFast, KeyBinding::new(KeyCode::Digit4));
        
        Self { bindings }
    }
    
    /// Get the binding for an action
    pub fn get(&self, action: GameAction) -> Option<&KeyBinding> {
        self.bindings.get(&action)
    }
    
    /// Set a new binding for an action
    pub fn set(&mut self, action: GameAction, binding: KeyBinding) {
        self.bindings.insert(action, binding);
    }
    
    /// Reset all bindings to defaults
    pub fn reset_to_defaults(&mut self) {
        *self = Self::create_defaults();
    }
    
    /// Check if an action is currently pressed
    pub fn action_pressed(&self, action: GameAction, keyboard: &ButtonInput<KeyCode>) -> bool {
        if let Some(binding) = self.get(action) {
            keyboard.pressed(binding.primary) || 
                binding.secondary.map_or(false, |key| keyboard.pressed(key))
        } else {
            false
        }
    }
    
    /// Check if an action was just pressed this frame
    pub fn action_just_pressed(&self, action: GameAction, keyboard: &ButtonInput<KeyCode>) -> bool {
        if let Some(binding) = self.get(action) {
            keyboard.just_pressed(binding.primary) || 
                binding.secondary.map_or(false, |key| keyboard.just_pressed(key))
        } else {
            false
        }
    }
    
    /// Check if a key is already bound to any action (for conflict detection)
    pub fn is_key_bound(&self, key: KeyCode, exclude_action: Option<GameAction>) -> Option<GameAction> {
        self.bindings.iter()
            .find(|(action, binding)| {
                if let Some(excluded) = exclude_action {
                    if **action == excluded {
                        return false;
                    }
                }
                binding.matches(key)
            })
            .map(|(action, _)| *action)
    }
    
    /// Get a human-readable string for a key
    pub fn key_name(key: KeyCode) -> &'static str {
        match key {
            KeyCode::Escape => "ESC",
            KeyCode::Enter => "ENTER",
            KeyCode::Space => "SPACE",
            KeyCode::Tab => "TAB",
            KeyCode::Backspace => "BACKSPACE",
            KeyCode::KeyA => "A",
            KeyCode::KeyB => "B",
            KeyCode::KeyC => "C",
            KeyCode::KeyD => "D",
            KeyCode::KeyE => "E",
            KeyCode::KeyF => "F",
            KeyCode::KeyG => "G",
            KeyCode::KeyH => "H",
            KeyCode::KeyI => "I",
            KeyCode::KeyJ => "J",
            KeyCode::KeyK => "K",
            KeyCode::KeyL => "L",
            KeyCode::KeyM => "M",
            KeyCode::KeyN => "N",
            KeyCode::KeyO => "O",
            KeyCode::KeyP => "P",
            KeyCode::KeyQ => "Q",
            KeyCode::KeyR => "R",
            KeyCode::KeyS => "S",
            KeyCode::KeyT => "T",
            KeyCode::KeyU => "U",
            KeyCode::KeyV => "V",
            KeyCode::KeyW => "W",
            KeyCode::KeyX => "X",
            KeyCode::KeyY => "Y",
            KeyCode::KeyZ => "Z",
            KeyCode::Digit1 => "1",
            KeyCode::Digit2 => "2",
            KeyCode::Digit3 => "3",
            KeyCode::Digit4 => "4",
            KeyCode::Digit5 => "5",
            KeyCode::Digit6 => "6",
            KeyCode::Digit7 => "7",
            KeyCode::Digit8 => "8",
            KeyCode::Digit9 => "9",
            KeyCode::Digit0 => "0",
            KeyCode::F1 => "F1",
            KeyCode::F2 => "F2",
            KeyCode::F3 => "F3",
            KeyCode::F4 => "F4",
            KeyCode::F5 => "F5",
            KeyCode::F6 => "F6",
            KeyCode::F7 => "F7",
            KeyCode::F8 => "F8",
            KeyCode::F9 => "F9",
            KeyCode::F10 => "F10",
            KeyCode::F11 => "F11",
            KeyCode::F12 => "F12",
            KeyCode::Minus => "-",
            KeyCode::Equal => "=",
            KeyCode::NumpadAdd => "NUM+",
            KeyCode::NumpadSubtract => "NUM-",
            KeyCode::NumpadMultiply => "NUM*",
            KeyCode::NumpadDivide => "NUM/",
            KeyCode::ArrowUp => "↑",
            KeyCode::ArrowDown => "↓",
            KeyCode::ArrowLeft => "←",
            KeyCode::ArrowRight => "→",
            _ => "???",
        }
    }
    
    /// Get display string for a binding
    pub fn binding_display(&self, action: GameAction) -> String {
        if let Some(binding) = self.get(action) {
            let primary = Self::key_name(binding.primary);
            if let Some(secondary) = binding.secondary {
                format!("{} / {}", primary, Self::key_name(secondary))
            } else {
                primary.to_string()
            }
        } else {
            "Unbound".to_string()
        }
    }
}

