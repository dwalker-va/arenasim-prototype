//! Headless match execution
//!
//! Runs arena matches without any graphical output, suitable for automated testing.

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use std::time::Duration;

use crate::combat::log::{CombatLog, CombatLogEventType, CombatantMetadata, MatchMetadata};
use crate::states::match_config::MatchConfig;
use crate::states::play_match::AbilityConfigPlugin;
// Use the stable systems API instead of importing internal functions directly
use crate::states::play_match::systems::{
    self, combatant_id, Combatant, FloatingTextState, GameRng, MatchCountdown, ShadowSightState,
    SimulationSpeed,
};

use super::config::HeadlessMatchConfig;

/// Result of a completed headless match
///
/// This struct provides programmatic access to match results for testing and analysis.
#[derive(Debug, Clone)]
pub struct MatchResult {
    /// The winning team (1 or 2), or None for a draw
    pub winner: Option<u8>,
    /// Total match duration in seconds (from gates opening to match end)
    pub match_time: f32,
    /// Combatant statistics from the match
    pub team1_combatants: Vec<CombatantResult>,
    /// Combatant statistics from the match
    pub team2_combatants: Vec<CombatantResult>,
    /// Random seed used (if deterministic mode)
    pub random_seed: Option<u64>,
}

/// Statistics for a single combatant after the match
#[derive(Debug, Clone)]
pub struct CombatantResult {
    /// Class name (e.g., "Warrior", "Mage")
    pub class_name: String,
    /// Maximum health
    pub max_health: f32,
    /// Health remaining at match end (0 if dead)
    pub final_health: f32,
    /// Whether this combatant survived
    pub survived: bool,
    /// Total damage dealt during the match
    pub damage_dealt: f32,
    /// Total damage taken during the match
    pub damage_taken: f32,
}

/// Resource to track headless match state
#[derive(Resource)]
pub struct HeadlessMatchState {
    /// Maximum match duration before declaring a draw
    pub max_duration: f32,
    /// Elapsed match time
    pub elapsed_time: f32,
    /// Custom output path for match log
    pub output_path: Option<String>,
    /// Whether the match has completed
    pub match_complete: bool,
    /// Random seed for deterministic simulation (if provided)
    pub random_seed: Option<u64>,
    /// Match result (populated when match completes)
    pub result: Option<MatchResult>,
}

/// Plugin for headless match execution
pub struct HeadlessPlugin {
    pub config: HeadlessMatchConfig,
}

impl Plugin for HeadlessPlugin {
    fn build(&self, app: &mut App) {
        let match_config = self
            .config
            .to_match_config()
            .expect("Invalid match configuration");

        app.insert_resource(match_config)
            .insert_resource(HeadlessMatchState {
                max_duration: self.config.max_duration_secs,
                elapsed_time: 0.0,
                output_path: self.config.output_path.clone(),
                match_complete: false,
                random_seed: self.config.random_seed,
                result: None,
            })
            .init_resource::<CombatLog>();

        // Configure combat system phase ordering
        systems::configure_combat_system_ordering(app);

        // Add core combat systems using the shared API (always run in headless mode)
        systems::add_core_combat_systems(app, || true);

        // Add headless-specific systems after combat resolution
        app.add_systems(Startup, headless_setup_match)
            .add_systems(
                Update,
                (headless_track_time, headless_check_match_end)
                    .chain()
                    .after(systems::CombatSystemPhase::CombatResolution),
            )
            .add_systems(PostUpdate, headless_exit_on_complete);
    }
}

/// Setup system for headless match
fn headless_setup_match(
    mut commands: Commands,
    config: Res<MatchConfig>,
    headless_state: Res<HeadlessMatchState>,
    mut combat_log: ResMut<CombatLog>,
) {
    // Clear and initialize combat log
    combat_log.clear();
    combat_log.log(
        CombatLogEventType::MatchEvent,
        "Match started (headless mode)!".to_string(),
    );

    // Initialize required resources
    commands.insert_resource(SimulationSpeed { multiplier: 1.0 });
    commands.insert_resource(MatchCountdown::default());
    commands.insert_resource(ShadowSightState::default());

    // Initialize GameRng with seed if provided (deterministic mode)
    let game_rng = match headless_state.random_seed {
        Some(seed) => {
            info!("Using deterministic RNG with seed: {}", seed);
            GameRng::from_seed(seed)
        }
        None => {
            info!("Using non-deterministic RNG (no seed provided)");
            GameRng::from_entropy()
        }
    };
    commands.insert_resource(game_rng);

    // Spawn combatants for Team 1
    let team1_spawn_x = -35.0;
    for (i, character_opt) in config.team1.iter().enumerate() {
        if let Some(character) = character_opt {
            combat_log.register_combatant(combatant_id(1, *character));
            let rogue_opener = config.team1_rogue_openers.get(i).copied().unwrap_or_default();
            let warlock_curse_prefs = config.team1_warlock_curse_prefs.get(i).cloned().unwrap_or_default();
            commands.spawn((
                Transform::from_xyz(team1_spawn_x, 1.0, (i as f32 - 1.0) * 3.0),
                Combatant::new_with_curse_prefs(1, *character, rogue_opener, warlock_curse_prefs),
                FloatingTextState {
                    next_pattern_index: 0,
                },
            ));
        }
    }

    // Spawn combatants for Team 2
    let team2_spawn_x = 35.0;
    for (i, character_opt) in config.team2.iter().enumerate() {
        if let Some(character) = character_opt {
            combat_log.register_combatant(combatant_id(2, *character));
            let rogue_opener = config.team2_rogue_openers.get(i).copied().unwrap_or_default();
            let warlock_curse_prefs = config.team2_warlock_curse_prefs.get(i).cloned().unwrap_or_default();
            commands.spawn((
                Transform::from_xyz(team2_spawn_x, 1.0, (i as f32 - 1.0) * 3.0),
                Combatant::new_with_curse_prefs(2, *character, rogue_opener, warlock_curse_prefs),
                FloatingTextState {
                    next_pattern_index: 0,
                },
            ));
        }
    }

    info!(
        "Headless match setup complete: Team 1 ({} members) vs Team 2 ({} members)",
        config.team1.len(),
        config.team2.len()
    );
}

/// Track elapsed combat time for the headless match (used for timeout detection).
///
/// Note: `combat_log.match_time` is updated by `combat_auto_attack` in combat_core.rs,
/// which runs from the start of the match (including prep phase). We only track
/// `elapsed_time` here for timeout purposes - it measures time since gates opened.
fn headless_track_time(
    time: Res<Time>,
    mut headless_state: ResMut<HeadlessMatchState>,
    countdown: Res<MatchCountdown>,
) {
    // Only track elapsed combat time after gates open (for timeout detection)
    if countdown.gates_opened {
        let dt = time.delta_secs();
        headless_state.elapsed_time += dt;
    }
}

/// Check if the match has ended (one or both teams eliminated, or timeout)
fn headless_check_match_end(
    combatants: Query<(&Combatant, &Transform)>,
    config: Res<MatchConfig>,
    combat_log: Res<CombatLog>,
    mut headless_state: ResMut<HeadlessMatchState>,
    countdown: Res<MatchCountdown>,
) {
    if headless_state.match_complete || !countdown.gates_opened {
        return;
    }

    // Check for timeout first
    if headless_state.elapsed_time >= headless_state.max_duration {
        info!(
            "Match timed out after {:.1}s - declaring DRAW",
            headless_state.elapsed_time
        );
        let result = build_match_result(&combatants, None, &headless_state);
        save_headless_match_log(&combatants, &config, &combat_log, None, &headless_state);
        headless_state.result = Some(result);
        headless_state.match_complete = true;
        return;
    }

    // Check team survival
    let team1_alive = combatants.iter().any(|(c, _)| c.team == 1 && c.is_alive());
    let team2_alive = combatants.iter().any(|(c, _)| c.team == 2 && c.is_alive());

    if !team1_alive || !team2_alive {
        let winner = if !team1_alive && !team2_alive {
            info!("Match ended in a DRAW (both teams eliminated simultaneously)!");
            None
        } else if team1_alive {
            info!("Match ended! Team 1 wins!");
            Some(1)
        } else {
            info!("Match ended! Team 2 wins!");
            Some(2)
        };

        let result = build_match_result(&combatants, winner, &headless_state);
        save_headless_match_log(&combatants, &config, &combat_log, winner, &headless_state);
        headless_state.result = Some(result);
        headless_state.match_complete = true;
    }
}

/// Build the MatchResult from current combatant state
fn build_match_result(
    combatants: &Query<(&Combatant, &Transform)>,
    winner: Option<u8>,
    headless_state: &HeadlessMatchState,
) -> MatchResult {
    let mut team1_combatants = Vec::new();
    let mut team2_combatants = Vec::new();

    for (combatant, _transform) in combatants.iter() {
        let result = CombatantResult {
            class_name: combatant.class.name().to_string(),
            max_health: combatant.max_health,
            final_health: combatant.current_health,
            survived: combatant.is_alive(),
            damage_dealt: combatant.damage_dealt,
            damage_taken: combatant.damage_taken,
        };

        if combatant.team == 1 {
            team1_combatants.push(result);
        } else {
            team2_combatants.push(result);
        }
    }

    MatchResult {
        winner,
        match_time: headless_state.elapsed_time,
        team1_combatants,
        team2_combatants,
        random_seed: headless_state.random_seed,
    }
}

/// Save the combat log to a file
fn save_headless_match_log(
    combatants: &Query<(&Combatant, &Transform)>,
    config: &Res<MatchConfig>,
    combat_log: &Res<CombatLog>,
    winner: Option<u8>,
    headless_state: &HeadlessMatchState,
) {
    // Collect metadata for all combatants
    let mut team1_metadata = Vec::new();
    let mut team2_metadata = Vec::new();

    for (combatant, transform) in combatants.iter() {
        let metadata = CombatantMetadata {
            class_name: combatant.class.name().to_string(),
            max_health: combatant.max_health,
            final_health: combatant.current_health,
            max_mana: combatant.max_mana,
            final_mana: combatant.current_mana,
            damage_dealt: combatant.damage_dealt,
            damage_taken: combatant.damage_taken,
            final_position: (
                transform.translation.x,
                transform.translation.y,
                transform.translation.z,
            ),
        };

        if combatant.team == 1 {
            team1_metadata.push(metadata);
        } else {
            team2_metadata.push(metadata);
        }
    }

    // Build match metadata
    let match_metadata = MatchMetadata {
        arena_name: config.map.name().to_string(),
        winner,
        team1: team1_metadata,
        team2: team2_metadata,
    };

    // Save to file (use custom output path if provided)
    match combat_log.save_to_file(&match_metadata, headless_state.output_path.as_deref()) {
        Ok(filename) => {
            println!("Match complete. Log saved to: {}", filename);
        }
        Err(e) => {
            eprintln!("Failed to save combat log: {}", e);
        }
    }
}

/// Exit the app when the match is complete
fn headless_exit_on_complete(headless_state: Res<HeadlessMatchState>, mut exit: EventWriter<AppExit>) {
    if headless_state.match_complete {
        exit.send(AppExit::Success);
    }
}

/// Run a headless match with the given configuration
pub fn run_headless_match(config: HeadlessMatchConfig) -> Result<(), String> {
    println!("Starting headless match simulation...");
    println!(
        "  Team 1: {:?}",
        config.team1
    );
    println!(
        "  Team 2: {:?}",
        config.team2
    );
    println!("  Map: {}", config.map);
    println!(
        "  Max duration: {:.0}s",
        config.max_duration_secs
    );

    App::new()
        // Minimal plugins - no window, no rendering
        .add_plugins(
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
                1.0 / 60.0,
            ))),
        )
        // Transform and hierarchy plugins needed for entity positions
        .add_plugins(TransformPlugin)
        .add_plugins(HierarchyPlugin)
        // Load ability definitions from config
        .add_plugins(AbilityConfigPlugin)
        // Our headless match plugin
        .add_plugins(HeadlessPlugin { config })
        .run();

    Ok(())
}
