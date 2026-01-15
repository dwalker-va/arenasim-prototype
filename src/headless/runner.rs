//! Headless match execution
//!
//! Runs arena matches without any graphical output, suitable for automated testing.

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;
use std::time::Duration;

use crate::combat::log::{CombatLog, CombatLogEventType, CombatantMetadata, MatchMetadata};
use crate::states::match_config::MatchConfig;
use crate::states::play_match::{
    acquire_targets, apply_pending_auras, check_interrupts, check_orb_pickups, combat_auto_attack,
    combatant_id, decide_abilities, move_projectiles, move_to_target, process_aura_breaks,
    process_casting, process_dot_ticks, process_interrupts, process_projectile_hits,
    regenerate_resources, track_shadow_sight_timer, update_auras, update_countdown, Combatant,
    FloatingTextState, MatchCountdown, ShadowSightState, SimulationSpeed,
};

use super::config::HeadlessMatchConfig;

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
            })
            .init_resource::<CombatLog>()
            .add_systems(Startup, headless_setup_match)
            // Phase 1: Resources and Auras
            .add_systems(
                Update,
                (
                    update_countdown,
                    regenerate_resources,
                    track_shadow_sight_timer,
                    process_dot_ticks,
                    update_auras,
                    apply_pending_auras,
                )
                    .chain(),
            )
            // Phase 2: Combat and Movement (first half)
            .add_systems(
                Update,
                (
                    process_aura_breaks,
                    acquire_targets,
                    check_orb_pickups,
                    decide_abilities,
                    check_interrupts,
                    process_interrupts,
                    process_casting,
                )
                    .chain()
                    .after(apply_pending_auras),
            )
            // Phase 2: Combat and Movement (second half)
            .add_systems(
                Update,
                (
                    move_projectiles,
                    process_projectile_hits,
                    move_to_target,
                )
                    .chain()
                    .after(process_casting),
            )
            // Phase 3: Combat resolution and Headless-specific systems
            .add_systems(
                Update,
                (
                    combat_auto_attack,
                    headless_check_match_end,
                    headless_track_time,
                )
                    .chain()
                    .after(move_to_target),
            )
            .add_systems(PostUpdate, headless_exit_on_complete);
    }
}

/// Setup system for headless match
fn headless_setup_match(mut commands: Commands, config: Res<MatchConfig>, mut combat_log: ResMut<CombatLog>) {
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

    // Spawn combatants for Team 1
    let team1_spawn_x = -35.0;
    for (i, character_opt) in config.team1.iter().enumerate() {
        if let Some(character) = character_opt {
            combat_log.register_combatant(combatant_id(1, *character));
            commands.spawn((
                Transform::from_xyz(team1_spawn_x, 1.0, (i as f32 - 1.0) * 3.0),
                Combatant::new(1, *character),
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
            commands.spawn((
                Transform::from_xyz(team2_spawn_x, 1.0, (i as f32 - 1.0) * 3.0),
                Combatant::new(2, *character),
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

/// Track elapsed time for the headless match
fn headless_track_time(
    time: Res<Time>,
    mut combat_log: ResMut<CombatLog>,
    mut headless_state: ResMut<HeadlessMatchState>,
    countdown: Res<MatchCountdown>,
) {
    // Only track time after gates open
    if countdown.gates_opened {
        let dt = time.delta_secs();
        headless_state.elapsed_time += dt;
        combat_log.match_time = headless_state.elapsed_time;
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
        save_headless_match_log(&combatants, &config, &combat_log, None, &headless_state);
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

        save_headless_match_log(&combatants, &config, &combat_log, winner, &headless_state);
        headless_state.match_complete = true;
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

    // Save to file
    match combat_log.save_to_file(&match_metadata) {
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
        // Our headless match plugin
        .add_plugins(HeadlessPlugin { config })
        .run();

    Ok(())
}
