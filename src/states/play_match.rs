//! Play Match Scene - 3D Combat Arena
//!
//! This module handles the active match simulation where combatants battle each other.
//! It implements a simple auto-battle system with the following features:
//!
//! ## Combat System
//! - **Target Acquisition**: Combatants automatically find the nearest alive enemy
//! - **Auto-Attacks**: Each combatant attacks their target based on their attack speed
//! - **Damage & Stats**: Tracks damage dealt/taken for each combatant
//! - **Win Conditions**: Match ends when all combatants of one team are eliminated
//!
//! ## Visual Representation
//! - 3D capsule meshes represent combatants, colored by class
//! - Health bars rendered above each combatant's head using 2D overlay
//! - Simple arena floor (30x30 plane)
//! - Isometric camera view
//!
//! ## Flow
//! 1. `setup_play_match`: Spawns arena, camera, lights, and combatants from `MatchConfig`
//! 2. Systems run each frame:
//!    - `update_play_match`: Handle ESC key to exit
//!    - `acquire_targets`: Find nearest enemy for each combatant
//!    - `combat_auto_attack`: Process attacks based on attack speed
//!    - `check_match_end`: Detect when match is over, transition to Results
//!    - `render_health_bars`: Draw 2D health bars over 3D combatants
//! 3. `cleanup_play_match`: Despawn all entities when exiting

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use super::match_config::{self, MatchConfig};
use super::GameState;

/// Marker component for all entities spawned in the Play Match scene.
/// Used for cleanup when exiting the scene.
#[derive(Component)]
pub struct PlayMatchEntity;

/// Core combatant component containing all combat state and stats.
#[derive(Component, Clone)]
pub struct Combatant {
    /// Team identifier (1 or 2)
    pub team: u8,
    /// Character class (Warrior, Mage, Rogue, Priest)
    pub class: match_config::CharacterClass,
    /// Maximum health points
    pub max_health: f32,
    /// Current health points (combatant dies when this reaches 0)
    pub current_health: f32,
    /// Base damage per attack
    pub attack_damage: f32,
    /// Attacks per second
    pub attack_speed: f32,
    /// Timer tracking time until next attack
    pub attack_timer: f32,
    /// Current target entity (None if no valid target)
    pub target: Option<Entity>,
    /// Total damage this combatant has dealt
    pub damage_dealt: f32,
    /// Total damage this combatant has taken
    pub damage_taken: f32,
}

impl Combatant {
    /// Create a new combatant with class-specific stats.
    pub fn new(team: u8, class: match_config::CharacterClass) -> Self {
        // Class-specific stats (health, damage, attack speed)
        let (max_health, attack_damage, attack_speed) = match class {
            match_config::CharacterClass::Warrior => (150.0, 12.0, 1.0),
            match_config::CharacterClass::Mage => (80.0, 20.0, 0.7),
            match_config::CharacterClass::Rogue => (100.0, 15.0, 1.3),
            match_config::CharacterClass::Priest => (90.0, 8.0, 0.8),
        };
        
        Self {
            team,
            class,
            max_health,
            current_health: max_health,
            attack_damage,
            attack_speed,
            attack_timer: 0.0,
            target: None,
            damage_dealt: 0.0,
            damage_taken: 0.0,
        }
    }
    
    /// Check if this combatant is alive (health > 0).
    pub fn is_alive(&self) -> bool {
        self.current_health > 0.0
    }
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
    pub survived: bool,
}

// ============================================================================
// Setup & Cleanup Systems
// ============================================================================

/// Setup system: Spawns the 3D arena, camera, lighting, and combatants.
/// 
/// This runs once when entering the PlayMatch state.
/// Reads the `MatchConfig` resource to determine team compositions.
pub fn setup_play_match(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    config: Res<MatchConfig>,
) {
    info!("Setting up Play Match scene with config: {:?}", *config);

    // Spawn 3D camera with isometric-ish view
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 20.0, 25.0).looking_at(Vec3::ZERO, Vec3::Y),
        PlayMatchEntity,
    ));

    // Add directional light (sun-like)
    commands.spawn((
        DirectionalLight {
            illuminance: 20000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        PlayMatchEntity,
    ));

    // Add ambient light for overall scene brightness
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.3, 0.3, 0.4),
        brightness: 300.0,
    });

    // Spawn arena floor - 30x30 unit plane
    let floor_size = 30.0;
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(floor_size, floor_size))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.25, 0.3),
            perceptual_roughness: 0.9,
            ..default()
        })),
        PlayMatchEntity,
    ));

    // Count class occurrences per team to apply darkening to duplicates
    use std::collections::HashMap;
    let mut team1_class_counts: HashMap<match_config::CharacterClass, usize> = HashMap::new();
    let mut team2_class_counts: HashMap<match_config::CharacterClass, usize> = HashMap::new();

    // Spawn Team 1 combatants (left side of arena)
    let team1_spawn_x = -10.0;
    for (i, character_opt) in config.team1.iter().enumerate() {
        if let Some(character) = character_opt {
            let count = *team1_class_counts.get(character).unwrap_or(&0);
            *team1_class_counts.entry(*character).or_insert(0) += 1;
            
            spawn_combatant(
                &mut commands,
                &mut meshes,
                &mut materials,
                1,
                *character,
                Vec3::new(team1_spawn_x, 1.0, (i as f32 - 1.0) * 3.0),
                count,
            );
        }
    }

    // Spawn Team 2 combatants (right side of arena)
    let team2_spawn_x = 10.0;
    for (i, character_opt) in config.team2.iter().enumerate() {
        if let Some(character) = character_opt {
            let count = *team2_class_counts.get(character).unwrap_or(&0);
            *team2_class_counts.entry(*character).or_insert(0) += 1;
            
            spawn_combatant(
                &mut commands,
                &mut meshes,
                &mut materials,
                2,
                *character,
                Vec3::new(team2_spawn_x, 1.0, (i as f32 - 1.0) * 3.0),
                count,
            );
        }
    }
}

/// Helper function to spawn a single combatant entity.
/// 
/// Creates a capsule mesh colored by class, with darker shades for duplicates.
/// The `duplicate_index` parameter determines how much to darken (0 = base color, 1+ = darkened).
fn spawn_combatant(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    team: u8,
    class: match_config::CharacterClass,
    position: Vec3,
    duplicate_index: usize,
) {
    // Get vibrant class colors for 3D visibility
    let base_color = match class {
        match_config::CharacterClass::Warrior => Color::srgb(0.9, 0.6, 0.3), // Orange/brown
        match_config::CharacterClass::Mage => Color::srgb(0.3, 0.6, 1.0),    // Bright blue
        match_config::CharacterClass::Rogue => Color::srgb(1.0, 0.9, 0.2),   // Bright yellow
        match_config::CharacterClass::Priest => Color::srgb(0.95, 0.95, 0.95), // White
    };
    
    // Apply darkening for duplicate classes (0.65 multiplier per duplicate)
    let darken_factor = 0.65f32.powi(duplicate_index as i32);
    let combatant_color = Color::srgb(
        base_color.to_srgba().red * darken_factor,
        base_color.to_srgba().green * darken_factor,
        base_color.to_srgba().blue * darken_factor,
    );

    // Create combatant mesh (capsule represents the body)
    let mesh = meshes.add(Capsule3d::new(0.5, 1.5));
    let material = materials.add(StandardMaterial {
        base_color: combatant_color,
        perceptual_roughness: 0.5, // More reflective for better color visibility
        metallic: 0.2, // Slight metallic sheen for color pop
        ..default()
    });

    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_translation(position),
        Combatant::new(team, class),
        PlayMatchEntity,
    ));
}

/// Cleanup system: Despawns all Play Match entities when exiting the state.
pub fn cleanup_play_match(
    mut commands: Commands,
    query: Query<Entity, With<PlayMatchEntity>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
    
    // Remove ambient light resource
    commands.remove_resource::<AmbientLight>();
}

// ============================================================================
// Update & Input Systems
// ============================================================================

/// Handle player input during the match.
/// Currently only handles ESC key to return to main menu.
pub fn update_play_match(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        next_state.set(GameState::MainMenu);
    }
}

/// Render 2D health bars above each living combatant's 3D position.
/// 
/// This system uses egui to draw health bars in screen space,
/// converting 3D world positions to 2D screen coordinates.
pub fn render_health_bars(
    mut contexts: EguiContexts,
    combatants: Query<(&Combatant, &Transform)>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
) {
    let ctx = contexts.ctx_mut();
    
    let Ok((camera, camera_transform)) = camera_query.get_single() else {
        return;
    };

    egui::Area::new(egui::Id::new("health_bars"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .show(ctx, |ui| {
            for (combatant, transform) in combatants.iter() {
                if !combatant.is_alive() {
                    continue;
                }

                // Project 3D position to 2D screen space
                let health_bar_offset = Vec3::new(0.0, 2.5, 0.0); // Above head
                let world_pos = transform.translation + health_bar_offset;
                
                if let Ok(screen_pos) = camera.world_to_viewport(camera_transform, world_pos) {
                    let health_percent = combatant.current_health / combatant.max_health;
                    
                    // Health bar dimensions
                    let bar_width = 50.0;
                    let bar_height = 6.0;
                    let bar_pos = egui::pos2(
                        screen_pos.x - bar_width / 2.0,
                        screen_pos.y - bar_height / 2.0,
                    );

                    // Background (dark gray)
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(bar_pos, egui::vec2(bar_width, bar_height)),
                        2.0,
                        egui::Color32::from_rgb(30, 30, 30),
                    );

                    // Health bar fill (color based on health %)
                    let health_color = if health_percent > 0.5 {
                        egui::Color32::from_rgb(0, 200, 0) // Green
                    } else if health_percent > 0.25 {
                        egui::Color32::from_rgb(255, 200, 0) // Yellow
                    } else {
                        egui::Color32::from_rgb(200, 0, 0) // Red
                    };

                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            bar_pos,
                            egui::vec2(bar_width * health_percent, bar_height),
                        ),
                        2.0,
                        health_color,
                    );

                    // Border
                    ui.painter().rect_stroke(
                        egui::Rect::from_min_size(bar_pos, egui::vec2(bar_width, bar_height)),
                        2.0,
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(200, 200, 200)),
                    );
                }
            }
        });
}

// ============================================================================
// Combat Systems
// ============================================================================

/// Target acquisition system: Each combatant finds the nearest alive enemy.
/// 
/// This runs every frame to update targets when:
/// - A combatant has no target
/// - Their current target has died
/// - A closer enemy becomes available (future enhancement)
pub fn acquire_targets(
    mut combatants: Query<(Entity, &mut Combatant, &Transform)>,
) {
    // Build list of all alive combatants with their info
    let alive_combatants: Vec<(Entity, u8, Vec3)> = combatants
        .iter()
        .filter(|(_, c, _)| c.is_alive())
        .map(|(entity, c, transform)| (entity, c.team, transform.translation))
        .collect();

    // For each combatant, ensure they have a valid target
    for (_entity, mut combatant, transform) in combatants.iter_mut() {
        if !combatant.is_alive() {
            combatant.target = None;
            continue;
        }

        // Check if current target is still valid (alive and on enemy team)
        let target_valid = combatant.target.and_then(|target_entity| {
            alive_combatants
                .iter()
                .find(|(e, _, _)| *e == target_entity)
                .filter(|(_, team, _)| *team != combatant.team)
        }).is_some();

        // If no valid target, find nearest enemy
        if !target_valid {
            let my_pos = transform.translation;
            let nearest_enemy = alive_combatants
                .iter()
                .filter(|(_, team, _)| *team != combatant.team)
                .min_by(|(_, _, pos_a), (_, _, pos_b)| {
                    let dist_a = my_pos.distance(*pos_a);
                    let dist_b = my_pos.distance(*pos_b);
                    dist_a.partial_cmp(&dist_b).unwrap()
                });

            combatant.target = nearest_enemy.map(|(entity, _, _)| *entity);
        }
    }
}

/// Auto-attack system: Process attacks based on attack speed timers.
/// 
/// Each combatant has an attack timer that counts up. When it reaches
/// the attack interval (1.0 / attack_speed), they attack their target.
/// 
/// Damage is applied immediately and stats are updated for both attacker and target.
pub fn combat_auto_attack(
    time: Res<Time>,
    mut combatants: Query<(Entity, &mut Combatant)>,
) {
    let dt = time.delta_secs();
    
    // Collect attacks that will happen this frame (attacker, target, damage)
    let mut attacks = Vec::new();
    
    for (attacker_entity, mut combatant) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }

        // Update attack timer
        combatant.attack_timer += dt;

        // Check if ready to attack and has a target
        let attack_interval = 1.0 / combatant.attack_speed;
        if combatant.attack_timer >= attack_interval {
            if let Some(target_entity) = combatant.target {
                attacks.push((attacker_entity, target_entity, combatant.attack_damage));
                combatant.attack_timer = 0.0;
            }
        }
    }

    // Apply damage to targets and track damage dealt
    let mut damage_dealt_updates: Vec<(Entity, f32)> = Vec::new();
    
    for (attacker_entity, target_entity, damage) in attacks {
        if let Ok((_, mut target)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                let actual_damage = damage.min(target.current_health);
                target.current_health = (target.current_health - damage).max(0.0);
                target.damage_taken += actual_damage;
                
                // Collect attacker damage for later update
                damage_dealt_updates.push((attacker_entity, actual_damage));
                
                if !target.is_alive() {
                    info!("Combatant died! Team {} {} eliminated", target.team, target.class.name());
                }
            }
        }
    }
    
    // Update attacker damage dealt stats
    for (attacker_entity, damage) in damage_dealt_updates {
        if let Ok((_, mut attacker)) = combatants.get_mut(attacker_entity) {
            attacker.damage_dealt += damage;
        }
    }
}

/// Check if the match has ended (one or both teams eliminated).
/// 
/// When the match ends:
/// 1. Determine winner (or draw if both teams die simultaneously)
/// 2. Collect final stats for all combatants
/// 3. Insert `MatchResults` resource for the Results scene
/// 4. Transition to Results state
pub fn check_match_end(
    combatants: Query<&Combatant>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let team1_alive = combatants.iter().any(|c| c.team == 1 && c.is_alive());
    let team2_alive = combatants.iter().any(|c| c.team == 2 && c.is_alive());

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
        
        // Collect final stats for all combatants
        let mut team1_stats = Vec::new();
        let mut team2_stats = Vec::new();
        
        for combatant in combatants.iter() {
            let stats = CombatantStats {
                class: combatant.class,
                damage_dealt: combatant.damage_dealt,
                damage_taken: combatant.damage_taken,
                survived: combatant.is_alive(),
            };
            
            if combatant.team == 1 {
                team1_stats.push(stats);
            } else {
                team2_stats.push(stats);
            }
        }
        
        // Store match results for the Results scene
        commands.insert_resource(MatchResults {
            winner,
            team1_combatants: team1_stats,
            team2_combatants: team2_stats,
        });
        
        next_state.set(GameState::Results);
    }
}

