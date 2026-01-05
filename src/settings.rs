//! Game settings and configuration
//!
//! Manages user preferences for graphics, audio, and other options.

use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use crate::keybindings::Keybindings;

/// User-configurable game settings
#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
pub struct GameSettings {
    pub window_mode: WindowModeOption,
    pub resolution: ResolutionOption,
    pub vsync: bool,
    pub keybindings: Keybindings,
}

/// Tracks whether settings have changed and require application restart
#[derive(Resource)]
pub struct PendingSettingsRestart {
    pub restart_required: bool,
    /// Store previous settings to detect what changed
    previous_settings: GameSettings,
}

impl Default for PendingSettingsRestart {
    fn default() -> Self {
        Self {
            restart_required: false,
            previous_settings: GameSettings::default(),
        }
    }
}

impl PendingSettingsRestart {
    /// Update with new settings and determine if restart is needed
    pub fn check_restart_needed(&mut self, new_settings: &GameSettings) -> bool {
        // Only window mode and resolution changes require restart
        let needs_restart = 
            self.previous_settings.window_mode != new_settings.window_mode ||
            self.previous_settings.resolution != new_settings.resolution;
        
        self.previous_settings = new_settings.clone();
        self.restart_required = needs_restart;
        
        needs_restart
    }
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            window_mode: WindowModeOption::Windowed,
            resolution: ResolutionOption::HD720,
            vsync: true,
            keybindings: Keybindings::default(),
        }
    }
}

impl GameSettings {
    /// Get the path to the settings file
    fn settings_path() -> PathBuf {
        // Store in the same directory as the executable for now
        // In production, you'd use directories::ProjectDirs for proper cross-platform support
        PathBuf::from("settings.ron")
    }

    /// Load settings from file, or return default if file doesn't exist
    pub fn load() -> Self {
        let path = Self::settings_path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(contents) => match ron::from_str(&contents) {
                    Ok(settings) => {
                        info!("Loaded settings from {:?}", path);
                        settings
                    }
                    Err(e) => {
                        warn!("Failed to parse settings file: {}", e);
                        Self::default()
                    }
                },
                Err(e) => {
                    warn!("Failed to read settings file: {}", e);
                    Self::default()
                }
            }
        } else {
            info!("No settings file found, using defaults");
            Self::default()
        }
    }

    /// Save settings to file
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::settings_path();
        let contents = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        fs::write(&path, contents)?;
        info!("Saved settings to {:?}", path);
        Ok(())
    }
}

/// Window mode options for the UI
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowModeOption {
    Windowed,
    BorderlessFullscreen,
}

impl WindowModeOption {
    pub fn to_bevy(&self) -> WindowMode {
        match self {
            WindowModeOption::Windowed => WindowMode::Windowed,
            WindowModeOption::BorderlessFullscreen => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            WindowModeOption::Windowed => "Windowed",
            WindowModeOption::BorderlessFullscreen => "Borderless Fullscreen",
        }
    }

    pub fn all() -> [WindowModeOption; 2] {
        [WindowModeOption::Windowed, WindowModeOption::BorderlessFullscreen]
    }
}

/// Resolution presets
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionOption {
    HD720,
    HD1080,
    QHD1440,
}

impl ResolutionOption {
    pub fn dimensions(&self) -> (f32, f32) {
        match self {
            ResolutionOption::HD720 => (1280.0, 720.0),
            ResolutionOption::HD1080 => (1920.0, 1080.0),
            ResolutionOption::QHD1440 => (2560.0, 1440.0),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ResolutionOption::HD720 => "1280 × 720",
            ResolutionOption::HD1080 => "1920 × 1080",
            ResolutionOption::QHD1440 => "2560 × 1440",
        }
    }

    pub fn all() -> [ResolutionOption; 3] {
        [
            ResolutionOption::HD720,
            ResolutionOption::HD1080,
            ResolutionOption::QHD1440,
        ]
    }
}

/// Plugin for managing game settings
pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        // Load settings from file
        let settings = GameSettings::load();
        
        // Also insert keybindings as a separate resource for easy access
        let keybindings = settings.keybindings.clone();
        
        app.insert_resource(settings.clone())
            .insert_resource(keybindings)
            .insert_resource(PendingSettingsRestart {
                restart_required: false,
                previous_settings: settings,
            })
            .add_systems(Update, (save_settings_on_change, apply_runtime_settings, sync_keybindings));
    }
}

/// System to save settings when they change
/// Determines if restart is required and applies runtime settings immediately
fn save_settings_on_change(
    settings: Res<GameSettings>,
    mut pending_restart: ResMut<PendingSettingsRestart>,
) {
    if settings.is_changed() && !settings.is_added() {
        // Check if this change requires restart (window mode or resolution)
        let needs_restart = pending_restart.check_restart_needed(&settings);
        
        // Save settings to file
        if let Err(e) = settings.save() {
            error!("Failed to save settings: {}", e);
        } else {
            if needs_restart {
                info!(
                    "Settings changed: {:?} @ {:?} (restart required)",
                    settings.window_mode,
                    settings.resolution
                );
            } else {
                info!("Settings changed and applied immediately");
            }
        }
    }
}

/// System to apply settings that can be changed at runtime (without restart)
/// Currently handles: VSync
fn apply_runtime_settings(
    settings: Res<GameSettings>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    pending_restart: Res<PendingSettingsRestart>,
) {
    // Only apply if settings changed AND it's not a restart-required change
    if settings.is_changed() && !settings.is_added() && !pending_restart.restart_required {
        if let Ok(mut window) = windows.get_single_mut() {
            // Apply VSync setting
            window.present_mode = if settings.vsync {
                PresentMode::AutoVsync
            } else {
                PresentMode::AutoNoVsync
            };
            
            info!("Applied VSync: {}", settings.vsync);
        }
    }
}

/// System to keep Keybindings resource in sync with GameSettings
fn sync_keybindings(
    settings: Res<GameSettings>,
    mut keybindings: ResMut<Keybindings>,
) {
    if settings.is_changed() && !settings.is_added() {
        *keybindings = settings.keybindings.clone();
        info!("Synced keybindings from settings");
    }
}

