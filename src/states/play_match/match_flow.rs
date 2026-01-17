//! Match Flow Systems
//!
//! Handles the overall flow of a match:
//! - Pre-combat countdown phase
//! - Time controls (pause, speed adjustment)
//! - Match end detection
//! - Victory celebration and transition to Results

use bevy::prelude::*;
use crate::combat::log::{CombatLog, CombatLogEventType, MatchMetadata, CombatantMetadata};
use crate::states::GameState;
use super::match_config::MatchConfig;
use super::components::*;

/// Update the pre-combat countdown timer.
/// 
/// During countdown:
/// - Tick down the timer
/// - Restore all combatants' mana to full (no penalty for buffing)
/// - Open gates when countdown reaches zero
pub fn update_countdown(
    time: Res<Time>,
    mut countdown: ResMut<MatchCountdown>,
    mut combat_log: ResMut<CombatLog>,
    mut combatants: Query<&mut Combatant>,
) {
    if countdown.gates_opened {
        return; // Gates already opened, nothing to do
    }
    
    let dt = time.delta_secs();
    countdown.time_remaining -= dt;
    
    // Restore all combatants' mana to full during countdown (every frame)
    // This ensures no penalty for pre-match buffing
    for mut combatant in combatants.iter_mut() {
        combatant.current_mana = combatant.max_mana;
    }
    
    // Check if countdown finished
    if countdown.time_remaining <= 0.0 {
        countdown.gates_opened = true;
        combat_log.log(
            CombatLogEventType::MatchEvent,
            "Gates open! Combat begins!".to_string()
        );
        info!("Gates opened - combat begins!");
    }
}

/// Animate gate bars lowering during the last 2 seconds of countdown
pub fn animate_gate_bars(
    countdown: Res<MatchCountdown>,
    mut gate_bars: Query<(&GateBar, &mut Transform, &mut Visibility)>,
) {
    // Gates lower during the last 2 seconds
    const GATE_OPEN_DURATION: f32 = 2.0;
    
    if countdown.gates_opened {
        // Gates fully open - hide all bars completely
        for (_gate_bar, mut transform, mut visibility) in gate_bars.iter_mut() {
            *visibility = Visibility::Hidden;
            transform.translation.y = -10.0; // Move far below ground to avoid any flicker
            transform.scale.y = 0.0;
        }
        return;
    }
    
    // Calculate how much the gates should be lowered
    if countdown.time_remaining <= GATE_OPEN_DURATION {
        let progress = 1.0 - (countdown.time_remaining / GATE_OPEN_DURATION); // 0.0 to 1.0
        
        for (gate_bar, mut transform, mut visibility) in gate_bars.iter_mut() {
            // Keep visible during lowering
            *visibility = Visibility::Visible;
            
            // Scale down the height
            let current_height = gate_bar.initial_height * (1.0 - progress);
            transform.scale.y = 1.0 - progress;
            // Adjust Y position so bars sink into the ground
            transform.translation.y = current_height / 2.0;
        }
    }
}

/// Handle time control keyboard shortcuts and apply time multiplier to simulation.
/// 
/// **Keyboard Shortcuts:**
/// - `Space`: Pause/Unpause
/// - `1`: 0.5x speed
/// - `2`: 1x speed (normal)
/// - `3`: 2x speed
/// - `4`: 3x speed
pub fn handle_time_controls(
    keybindings: Res<crate::keybindings::Keybindings>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut sim_speed: ResMut<SimulationSpeed>,
    mut time: ResMut<Time<Virtual>>,
) {
    use crate::keybindings::GameAction;
    
    let mut speed_changed = false;
    let old_multiplier = sim_speed.multiplier;
    
    // Pause/Play toggle
    if keybindings.action_just_pressed(GameAction::PausePlay, &keyboard) {
        if sim_speed.is_paused() {
            sim_speed.multiplier = 1.0; // Resume at normal speed
        } else {
            sim_speed.multiplier = 0.0; // Pause
        }
        speed_changed = true;
    }
    
    // Speed presets
    if keybindings.action_just_pressed(GameAction::SpeedSlow, &keyboard) {
        sim_speed.multiplier = 0.5;
        speed_changed = true;
    }
    if keybindings.action_just_pressed(GameAction::SpeedNormal, &keyboard) {
        sim_speed.multiplier = 1.0;
        speed_changed = true;
    }
    if keybindings.action_just_pressed(GameAction::SpeedFast, &keyboard) {
        sim_speed.multiplier = 2.0;
        speed_changed = true;
    }
    if keybindings.action_just_pressed(GameAction::SpeedVeryFast, &keyboard) {
        sim_speed.multiplier = 3.0;
        speed_changed = true;
    }
    
    // Apply speed to virtual time if changed
    if speed_changed {
        time.set_relative_speed(sim_speed.multiplier);
        
        if sim_speed.is_paused() {
            info!("Simulation PAUSED");
        } else if old_multiplier == 0.0 {
            info!("Simulation RESUMED at {}x speed", sim_speed.multiplier);
        } else {
            info!("Simulation speed changed to {}x", sim_speed.multiplier);
        }
    }
}

/// Check if the match has ended (one or both teams eliminated).
/// 
/// When the match ends:
/// 1. Determine winner (or draw if both teams die simultaneously)
/// 2. Collect final stats for all combatants
/// 3. Save combat log to file for debugging
/// 4. Insert `MatchResults` resource for the Results scene
/// 5. Start victory celebration (5 second countdown before transitioning)
pub fn check_match_end(
    combatants: Query<(Entity, &Combatant, &Transform)>,
    config: Res<MatchConfig>,
    combat_log: Res<CombatLog>,
    celebration: Option<Res<VictoryCelebration>>,
    projectiles: Query<Entity, With<Projectile>>,
    spell_effects: Query<Entity, With<SpellImpactEffect>>,
    mut commands: Commands,
) {
    // If celebration is already active, don't check for match end again
    if celebration.is_some() {
        return;
    }
    
    let team1_alive = combatants.iter().any(|(_, c, _)| c.team == 1 && c.is_alive());
    let team2_alive = combatants.iter().any(|(_, c, _)| c.team == 2 && c.is_alive());

    if !team1_alive || !team2_alive {
        // Determine winner: None if both dead (draw), otherwise winning team
        let winner = if !team1_alive && !team2_alive {
            info!("Match ended in a DRAW!");
            None
        } else if team1_alive {
            info!("Match ended! Team 1 wins!");
            Some(1)
        } else {
            info!("Match ended! Team 2 wins!");
            Some(2)
        };
        
        // Collect final stats for all combatants (for Results scene)
        let mut team1_stats = Vec::new();
        let mut team2_stats = Vec::new();
        
        // Collect metadata for combat log saving (with position data)
        let mut team1_metadata = Vec::new();
        let mut team2_metadata = Vec::new();
        
        for (entity, combatant, transform) in combatants.iter() {
            let stats = CombatantStats {
                class: combatant.class,
                damage_dealt: combatant.damage_dealt,
                damage_taken: combatant.damage_taken,
                healing_done: combatant.healing_done,
                survived: combatant.is_alive(),
            };
            
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
                team1_stats.push(stats);
                team1_metadata.push(metadata);
            } else {
                team2_stats.push(stats);
                team2_metadata.push(metadata);
            }
            
            // Mark winners as celebrating (for bounce animation)
            if combatant.is_alive() && Some(combatant.team) == winner {
                // Stagger bounce timing for visual variety
                let bounce_offset = (team1_stats.len() + team2_stats.len()) as f32 * 0.2;
                commands.entity(entity).insert(Celebrating { bounce_offset });
            }
            
            // Cancel any active casts to avoid frozen cast bars during celebration
            commands.entity(entity).remove::<CastingState>();
        }
        
        // Despawn all active projectiles to avoid frozen projectiles during celebration
        for projectile_entity in projectiles.iter() {
            commands.entity(projectile_entity).despawn_recursive();
        }
        
        // Despawn all active spell impact effects (e.g., Mind Blast shadow spheres)
        for effect_entity in spell_effects.iter() {
            commands.entity(effect_entity).despawn_recursive();
        }
        
        // Save combat log to file for debugging
        let match_metadata = MatchMetadata {
            arena_name: config.map.name().to_string(),
            winner,
            team1: team1_metadata,
            team2: team2_metadata,
        };
        
        match combat_log.save_to_file(&match_metadata, None) {
            Ok(filename) => {
                info!("Combat log saved to: {}", filename);
            }
            Err(e) => {
                error!("Failed to save combat log: {}", e);
            }
        }
        
        // Start victory celebration (5 seconds before transitioning to Results)
        commands.insert_resource(VictoryCelebration {
            winner,
            time_remaining: 5.0,
            match_results: MatchResults {
                winner,
                team1_combatants: team1_stats,
                team2_combatants: team2_stats,
            },
        });
        
        info!("Victory celebration started! {} seconds", 5.0);
    }
}

/// Update victory celebration: animate winners bouncing and countdown to Results.
/// 
/// During celebration (5 seconds):
/// - Winning combatants bounce up and down
/// - Victory text is displayed (via rendering system)
/// - Timer counts down
/// 
/// When timer reaches 0:
/// - Store match results for Results scene
/// - Transition to Results state
pub fn update_victory_celebration(
    time: Res<Time>,
    celebration: Option<ResMut<VictoryCelebration>>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<GameState>>,
    mut celebrating_combatants: Query<(&mut Transform, &Celebrating)>,
) {
    // Only run if celebration is active
    let Some(mut celebration) = celebration else {
        return;
    };
    let dt = time.delta_secs();
    celebration.time_remaining -= dt;
    
    // Animate celebrating combatants (bounce up and down)
    let celebration_time = 5.0 - celebration.time_remaining; // Elapsed time
    for (mut transform, celebrating) in celebrating_combatants.iter_mut() {
        // Sine wave for smooth bounce (frequency = 2 Hz for lively bounce)
        let bounce_time = celebration_time + celebrating.bounce_offset;
        let bounce_height = (bounce_time * std::f32::consts::TAU * 2.0).sin().max(0.0) * 0.8;
        
        // Set Y position (base height = 1.0 + bounce)
        transform.translation.y = 1.0 + bounce_height;
    }
    
    // Check if celebration finished
    if celebration.time_remaining <= 0.0 {
        // Store match results for Results scene
        commands.insert_resource(celebration.match_results.clone());
        
        // Transition to Results
        next_state.set(GameState::Results);
        info!("Victory celebration complete - transitioning to Results");
    }
}

