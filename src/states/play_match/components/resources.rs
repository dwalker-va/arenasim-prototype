use bevy::prelude::*;
use bevy_egui::egui;
use rand::prelude::*;
use rand::rngs::StdRng;
use super::super::match_config;

// ============================================================================
// Resources & Camera
// ============================================================================

/// Seeded random number generator for deterministic match simulation.
///
/// When a seed is provided (e.g., via headless config), the same seed will
/// always produce the same match outcome. Without a seed, uses system entropy.
#[derive(Resource)]
pub struct GameRng {
    rng: StdRng,
    /// The seed used to initialize this RNG (if deterministic)
    pub seed: Option<u64>,
}

impl GameRng {
    /// Create a new GameRng with a specific seed for deterministic behavior
    pub fn from_seed(seed: u64) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
            seed: Some(seed),
        }
    }

    /// Create a new GameRng with random entropy (non-deterministic)
    pub fn from_entropy() -> Self {
        Self {
            rng: StdRng::from_entropy(),
            seed: None,
        }
    }

    /// Generate a random f32 in the range [0.0, 1.0)
    pub fn random_f32(&mut self) -> f32 {
        self.rng.gen()
    }

    /// Generate a random f32 in the given range
    pub fn random_range(&mut self, min: f32, max: f32) -> f32 {
        min + self.random_f32() * (max - min)
    }
}

impl Default for GameRng {
    fn default() -> Self {
        Self::from_entropy()
    }
}

/// Controls the speed of combat simulation
#[derive(Resource)]
pub struct SimulationSpeed {
    pub multiplier: f32,
}

/// Display settings for the match UI (can be toggled during match)
#[derive(Resource)]
pub struct DisplaySettings {
    /// Whether to show aura icons below combatant health bars
    pub show_aura_icons: bool,
}

impl Default for SimulationSpeed {
    fn default() -> Self {
        Self { multiplier: 1.0 }
    }
}

impl SimulationSpeed {
    pub fn is_paused(&self) -> bool {
        self.multiplier == 0.0
    }
}

/// Which panel view is currently active in the combat log area
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq)]
pub enum CombatPanelView {
    CombatLog,
    #[default]
    Timeline,
}

/// Resource storing loaded spell icon textures for egui rendering in the timeline.
/// Maps ability name to egui TextureId for efficient icon display.
#[derive(Resource, Default)]
pub struct SpellIcons {
    /// Map of ability name to egui texture ID
    pub textures: std::collections::HashMap<String, egui::TextureId>,
    /// Whether icons have been loaded
    pub loaded: bool,
}

/// Resource storing the Bevy image handles for spell icons.
/// These are kept alive to prevent the assets from being unloaded.
#[derive(Resource, Default)]
pub struct SpellIconHandles {
    pub handles: Vec<(String, Handle<Image>)>,
}

/// Camera control modes
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraMode {
    /// Follow the center of all combatants
    FollowCenter,
    /// Follow a specific combatant
    FollowCombatant(Entity),
    /// Manual camera control
    Manual,
}

/// Camera controller state
#[derive(Resource)]
pub struct CameraController {
    pub mode: CameraMode,
    pub zoom_distance: f32,      // Distance from target
    pub pitch: f32,              // Rotation around X-axis (up/down)
    pub yaw: f32,                // Rotation around Y-axis (left/right)
    pub manual_target: Vec3,     // Look-at point for manual mode
    pub is_dragging: bool,       // Mouse drag state
    pub last_mouse_pos: Option<Vec2>,
    pub keyboard_movement: Vec3, // WASD movement delta this frame
}

impl Default for CameraController {
    fn default() -> Self {
        Self {
            mode: CameraMode::FollowCenter,
            zoom_distance: 60.0,
            pitch: 38.7f32.to_radians(), // ~40 degrees
            yaw: 0.0,
            manual_target: Vec3::ZERO,
            is_dragging: false,
            last_mouse_pos: None,
            keyboard_movement: Vec3::ZERO,
        }
    }
}

/// Match countdown state - tracks the pre-combat countdown phase
#[derive(Resource)]
pub struct MatchCountdown {
    /// Time remaining in countdown (in seconds). When <= 0, gates open and combat starts.
    pub time_remaining: f32,
    /// Whether the gates have opened (combat has started)
    pub gates_opened: bool,
}

impl Default for MatchCountdown {
    fn default() -> Self {
        Self {
            time_remaining: 10.0, // 10 second countdown as per design doc
            gates_opened: false,
        }
    }
}

/// Resource tracking Shadow Sight orb spawn state.
/// Shadow Sight orbs spawn after extended combat to break stealth stalemates.
#[derive(Resource)]
pub struct ShadowSightState {
    /// Time elapsed since gates opened
    pub combat_time: f32,
    /// Whether orbs have been spawned
    pub orbs_spawned: bool,
}

impl Default for ShadowSightState {
    fn default() -> Self {
        Self {
            combat_time: 0.0,
            orbs_spawned: false,
        }
    }
}

/// Victory celebration state - tracks post-match victory animation
#[derive(Resource)]
pub struct VictoryCelebration {
    /// Which team won (None = draw)
    pub winner: Option<u8>,
    /// Time remaining in celebration (in seconds). When <= 0, transition to Results.
    pub time_remaining: f32,
    /// Stored match results to pass to Results scene
    pub match_results: MatchResults,
}

/// Resource containing the final results of a match.
/// Inserted when the match ends, consumed by the Results scene.
#[derive(Resource, Clone)]
pub struct MatchResults {
    /// Winner: None = draw, Some(1) = team 1, Some(2) = team 2
    pub winner: Option<u8>,
    /// Stats for all Team 1 combatants
    pub team1_combatants: Vec<CombatantStats>,
    /// Stats for all Team 2 combatants
    pub team2_combatants: Vec<CombatantStats>,
}

/// Statistics for a single combatant at the end of a match.
#[derive(Clone)]
pub struct CombatantStats {
    pub class: match_config::CharacterClass,
    pub damage_dealt: f32,
    pub damage_taken: f32,
    pub healing_done: f32,
    pub survived: bool,
}
