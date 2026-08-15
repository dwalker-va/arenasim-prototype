//! Game settings and configuration
//!
//! Manages user preferences for graphics, audio, and other options.

use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use crate::keybindings::Keybindings;

/// User-configurable game settings
#[derive(Resource, Clone, Debug, Serialize, Deserialize)]
pub struct GameSettings {
    pub window_mode: WindowModeOption,
    pub resolution: ResolutionOption,
    pub vsync: bool,
    pub keybindings: Keybindings,
    /// Whether to show aura icons below combatant health bars (default: true)
    #[serde(default = "default_show_aura_icons")]
    pub show_aura_icons: bool,
    /// Whether the combat log / timeline panel is open (default: false — it is
    /// a diagnostic tool, not part of the default spectator presentation)
    #[serde(default = "default_show_combat_panel")]
    pub show_combat_panel: bool,
    /// Whether the in-match kill-call markers are shown on the team frames
    /// (default: true).
    ///
    /// Unlike the combat panel this follows in shape, the call display defaults
    /// ON. It was briefly off, on the theory that it is an experimentation
    /// affordance rather than part of the spectator view — but the markers show
    /// what each team is focusing, which is match state a watcher wants, and a
    /// control nobody can see is a control nobody uses. The toggle now exists
    /// to get a clean view, not to opt in.
    #[serde(default = "default_show_call_display")]
    pub show_call_display: bool,
}

fn default_show_aura_icons() -> bool {
    true
}

fn default_show_combat_panel() -> bool {
    false
}

fn default_show_call_display() -> bool {
    true
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
            show_aura_icons: true,
            show_combat_panel: false,
            show_call_display: true,
        }
    }
}

impl GameSettings {
    /// Get the path to the settings file
    fn settings_path() -> PathBuf {
        crate::paths::settings_path()
    }

    /// Load settings from file, or return default if file doesn't exist
    pub fn load() -> Self {
        let path = Self::settings_path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(contents) => match ron::from_str::<GameSettings>(&contents) {
                    Ok(mut settings) => {
                        // Fill in any missing keybindings (for newly added actions)
                        settings.keybindings.fill_missing_defaults();
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
        self.save_to(&Self::settings_path())
    }

    /// Save settings to an explicit path, creating its parent directory first —
    /// an installed build's per-user data directory does not exist yet.
    fn save_to(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            // Empty in a checkout, where the path is the bare `settings.ron`.
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        let contents = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        fs::write(path, contents)?;
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
        if let Ok(mut window) = windows.single_mut() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_display_flag_round_trips() {
        let mut settings = GameSettings::default();
        assert!(settings.show_call_display, "call display defaults to on");
        settings.show_call_display = false;

        let serialized = ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default())
            .expect("settings serialize");
        let loaded: GameSettings = ron::from_str(&serialized).expect("settings deserialize");

        assert!(!loaded.show_call_display);
    }

    /// An installed build's per-user data directory does not exist before the
    /// first save, so the save has to create it rather than failing.
    #[test]
    fn saving_creates_the_parent_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ArenaSim/settings.ron");

        GameSettings::default().save_to(&path).expect("save settings");

        let contents = fs::read_to_string(&path).expect("read settings back");
        ron::from_str::<GameSettings>(&contents).expect("settings deserialize");
    }

    /// A save that cannot be written must surface an error rather than panic —
    /// `save_settings_on_change` logs it and the game carries on.
    #[test]
    fn saving_to_an_unwritable_location_reports_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A regular file where a directory would have to be: nothing below it
        // can be created or written.
        let blocker = dir.path().join("not-a-directory");
        fs::write(&blocker, "").expect("write blocker");

        assert!(
            GameSettings::default().save_to(&blocker.join("settings.ron")).is_err(),
            "an unwritable destination should report an error"
        );
    }

    /// A settings file written before the field existed must still load — the
    /// `#[serde(default)]` path. Emulated by stripping the field back out of a
    /// freshly serialized payload.
    #[test]
    fn settings_written_before_the_field_existed_still_load() {
        let settings = GameSettings::default();
        let serialized = ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default())
            .expect("settings serialize");

        let legacy: String = serialized
            .lines()
            .filter(|line| !line.contains("show_call_display"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !legacy.contains("show_call_display"),
            "legacy payload should not mention the new field"
        );

        let loaded: GameSettings = ron::from_str(&legacy).expect("legacy settings deserialize");
        assert!(loaded.show_call_display, "missing field falls back to the default");
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

