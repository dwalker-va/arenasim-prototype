//! Play Match Scene - 3D Combat Arena
//!
//! This module handles the active match simulation where combatants battle each other.
//! Inspired by World of Warcraft's combat mechanics, it features:
//!
//! ## Combat System
//! - **Target Acquisition**: Combatants automatically find the nearest alive enemy
//! - **Movement**: Combatants move towards targets if out of range
//! - **Range Mechanics**: Melee attacks require being in melee range (2.5 units)
//! - **Auto-Attacks**: Each combatant attacks when in range, based on attack speed
//! - **Damage & Stats**: Tracks damage dealt/taken for each combatant
//! - **Win Conditions**: Match ends when all combatants of one team are eliminated
//!
//! ## Visual Representation
//! - 3D capsule meshes represent combatants, colored by class
//! - Health bars rendered above each combatant's head using 2D overlay
//! - Combatants rotate to face their targets
//! - Simple arena floor (30x30 plane)
//! - Isometric camera view
//!
//! ## Flow
//! 1. `setup_play_match`: Spawns arena, camera, lights, and combatants from `MatchConfig`
//! 2. Systems run each frame:
//!    - `update_play_match`: Handle ESC key to exit
//!    - `acquire_targets`: Find nearest enemy for each combatant
//!    - `move_to_target`: Move combatants towards targets if out of range
//!    - `combat_auto_attack`: Process attacks when in range, based on attack speed
//!    - `check_match_end`: Detect when match is over, transition to Results
//!    - `render_health_bars`: Draw 2D health bars over 3D combatants
//! 3. `cleanup_play_match`: Despawn all entities when exiting

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use super::match_config::{self, MatchConfig};
use super::GameState;
use crate::combat::log::{CombatLog, CombatLogEventType, PositionData, MatchMetadata, CombatantMetadata};

// ============================================================================
// Constants
// ============================================================================

/// Melee attack range in units. Combatants must be within this distance to auto-attack.
/// Similar to WoW's melee range of ~5 yards.
const MELEE_RANGE: f32 = 2.5;

/// Distance threshold for stopping movement (slightly less than melee range to avoid jitter)
const STOP_DISTANCE: f32 = 2.0;

// ============================================================================
// Components
// ============================================================================

/// Marker component for all entities spawned in the Play Match scene.
/// Used for cleanup when exiting the scene.
#[derive(Component)]
pub struct PlayMatchEntity;

/// Resource type for combatants (Mana, Energy, Rage).
/// Different classes use different resources with different mechanics.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResourceType {
    /// Mana - Used by Mages and Priests. Regenerates over time. Starts full.
    Mana,
    /// Energy - Used by Rogues. Regenerates rapidly. Starts full. Caps at 100.
    Energy,
    /// Rage - Used by Warriors. Starts at 0. Builds from auto-attacks and taking damage.
    Rage,
}

/// Floating combat text component for damage/healing numbers.
/// These appear above combatants and float upward before fading out.
#[derive(Component)]
pub struct FloatingCombatText {
    /// World position where the text is anchored
    pub world_position: Vec3,
    /// The text to display (damage/healing amount)
    pub text: String,
    /// Color of the text (white for auto-attacks, yellow for abilities, green for healing)
    pub color: egui::Color32,
    /// Time remaining before text disappears (in seconds)
    pub lifetime: f32,
    /// Vertical offset accumulated over time (makes text float upward)
    pub vertical_offset: f32,
}

/// Core combatant component containing all combat state and stats.
#[derive(Component, Clone)]
pub struct Combatant {
    /// Team identifier (1 or 2)
    pub team: u8,
    /// Character class (Warrior, Mage, Rogue, Priest)
    pub class: match_config::CharacterClass,
    /// Resource type (Mana, Energy, Rage)
    pub resource_type: ResourceType,
    /// Maximum health points
    pub max_health: f32,
    /// Current health points (combatant dies when this reaches 0)
    pub current_health: f32,
    /// Maximum mana/resource points
    pub max_mana: f32,
    /// Current mana/resource points (used to cast abilities)
    pub current_mana: f32,
    /// Mana regeneration per second
    pub mana_regen: f32,
    /// Base damage per attack
    pub attack_damage: f32,
    /// Attacks per second
    pub attack_speed: f32,
    /// Timer tracking time until next attack
    pub attack_timer: f32,
    /// Base movement speed in units per second (modified by auras/debuffs)
    pub base_movement_speed: f32,
    /// Current target entity (None if no valid target)
    pub target: Option<Entity>,
    /// Total damage this combatant has dealt
    pub damage_dealt: f32,
    /// Total damage this combatant has taken
    pub damage_taken: f32,
    /// Total healing this combatant has done
    pub healing_done: f32,
    /// Bonus damage for the next auto-attack (from abilities like Heroic Strike)
    pub next_attack_bonus_damage: f32,
    /// Whether this combatant is currently stealthed (Rogues only)
    pub stealthed: bool,
}

impl Combatant {
    /// Create a new combatant with class-specific stats.
    pub fn new(team: u8, class: match_config::CharacterClass) -> Self {
        // Class-specific stats (resource_type, health, max_resource, resource_regen, starting_resource, damage, attack speed, movement speed)
        let (resource_type, max_health, max_resource, resource_regen, starting_resource, attack_damage, attack_speed, movement_speed) = match class {
            match_config::CharacterClass::Warrior => (ResourceType::Rage, 150.0, 100.0, 0.0, 0.0, 12.0, 1.0, 5.0),  // Rage: starts at 0
            match_config::CharacterClass::Mage => (ResourceType::Mana, 80.0, 200.0, 10.0, 200.0, 20.0, 0.7, 4.5),   // Mana: starts full
            match_config::CharacterClass::Rogue => (ResourceType::Energy, 100.0, 100.0, 20.0, 100.0, 15.0, 1.3, 6.0), // Energy: starts full, fast regen
            match_config::CharacterClass::Priest => (ResourceType::Mana, 90.0, 150.0, 8.0, 150.0, 8.0, 0.8, 5.0),    // Mana: starts full
        };
        
        // Rogues start stealthed
        let stealthed = class == match_config::CharacterClass::Rogue;
        
        Self {
            team,
            class,
            resource_type,
            max_health,
            current_health: max_health,
            max_mana: max_resource,
            current_mana: starting_resource,
            mana_regen: resource_regen,
            attack_damage,
            attack_speed,
            attack_timer: 0.0,
            base_movement_speed: movement_speed,
            target: None,
            damage_dealt: 0.0,
            damage_taken: 0.0,
            healing_done: 0.0,
            next_attack_bonus_damage: 0.0,
            stealthed,
        }
    }
    
    /// Check if this combatant is alive (health > 0).
    pub fn is_alive(&self) -> bool {
        self.current_health > 0.0
    }
    
    /// Check if this combatant is in range to attack the target position.
    pub fn in_attack_range(&self, my_position: Vec3, target_position: Vec3) -> bool {
        my_position.distance(target_position) <= MELEE_RANGE
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
    pub healing_done: f32,
    pub survived: bool,
}

/// Component tracking an active cast in progress.
#[derive(Component)]
pub struct CastingState {
    /// The ability being cast
    pub ability: AbilityType,
    /// Time remaining until cast completes (in seconds)
    pub time_remaining: f32,
    /// Target entity for the ability (if single-target)
    pub target: Option<Entity>,
}

/// Component tracking active auras/debuffs on a combatant.
#[derive(Component, Default)]
pub struct ActiveAuras {
    pub auras: Vec<Aura>,
}

/// Temporary component for pending auras to be applied.
/// Used to avoid borrow checker issues when applying auras during casting.
#[derive(Component)]
pub struct AuraPending {
    pub target: Entity,
    pub aura: Aura,
}

/// An active aura/debuff effect on a combatant.
#[derive(Clone)]
pub struct Aura {
    /// Type of aura effect
    pub effect_type: AuraType,
    /// Time remaining before the aura expires (in seconds)
    pub duration: f32,
    /// Magnitude of the effect (e.g., 0.7 = 30% slow)
    pub magnitude: f32,
}

/// Types of aura effects.
#[derive(Clone, PartialEq, Debug)]
pub enum AuraType {
    /// Reduces movement speed by a percentage (magnitude = multiplier, e.g., 0.7 = 30% slow)
    MovementSpeedSlow,
    // Future: Stun, Root, Silence, Damage-over-time, Healing-over-time, etc.
}

/// Enum representing available abilities.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AbilityType {
    Frostbolt,
    FlashHeal,
    HeroicStrike,
    Ambush,
    // Future: Fireball, Backstab, etc.
}

impl AbilityType {
    /// Get ability definition (cast time, range, cost, etc.)
    pub fn definition(&self) -> AbilityDefinition {
        match self {
            AbilityType::Frostbolt => AbilityDefinition {
                name: "Frostbolt",
                cast_time: 2.5,
                range: 30.0,
                mana_cost: 20.0,
                cooldown: 0.0,
                damage_min: 25.0,
                damage_max: 30.0,
                healing_min: 0.0,
                healing_max: 0.0,
                applies_aura: Some((AuraType::MovementSpeedSlow, 5.0, 0.7)), // 30% slow for 5s
            },
            AbilityType::FlashHeal => AbilityDefinition {
                name: "Flash Heal",
                cast_time: 1.5,
                range: 40.0, // Longer range than Frostbolt
                mana_cost: 25.0,
                cooldown: 0.0,
                damage_min: 0.0,
                damage_max: 0.0,
                healing_min: 30.0,
                healing_max: 40.0,
                applies_aura: None,
            },
            AbilityType::HeroicStrike => AbilityDefinition {
                name: "Heroic Strike",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE,
                mana_cost: 15.0, // Costs 15 Rage
                cooldown: 0.0, // No cooldown
                damage_min: 0.0, // No direct damage - enhances next auto-attack
                damage_max: 0.0,
                healing_min: 0.0,
                healing_max: 0.0,
                applies_aura: None,
            },
            AbilityType::Ambush => AbilityDefinition {
                name: "Ambush",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE,
                mana_cost: 60.0, // High energy cost
                cooldown: 0.0,
                damage_min: 50.0, // High burst damage
                damage_max: 60.0,
                healing_min: 0.0,
                healing_max: 0.0,
                applies_aura: None,
            },
        }
    }
    
    /// Check if a combatant can cast this ability (has mana, in range, not casting, etc.)
    pub fn can_cast(&self, caster: &Combatant, target_position: Vec3, caster_position: Vec3) -> bool {
        let def = self.definition();
        
        // Check mana/resource
        if caster.current_mana < def.mana_cost {
            return false;
        }
        
        // Check range
        let distance = caster_position.distance(target_position);
        if distance > def.range {
            return false;
        }
        
        // Ambush requires stealth
        if matches!(self, AbilityType::Ambush) && !caster.stealthed {
            return false;
        }
        
        true
    }
}

/// Ability definition with all parameters.
pub struct AbilityDefinition {
    pub name: &'static str,
    /// Cast time in seconds (0.0 = instant)
    pub cast_time: f32,
    /// Maximum range in units
    pub range: f32,
    /// Mana cost to cast
    pub mana_cost: f32,
    /// Cooldown after cast (in seconds)
    pub cooldown: f32,
    /// Minimum damage dealt (0 for healing spells)
    pub damage_min: f32,
    /// Maximum damage dealt (0 for healing spells)
    pub damage_max: f32,
    /// Minimum healing done (0 for damage spells)
    pub healing_min: f32,
    /// Maximum healing done (0 for damage spells)
    pub healing_max: f32,
    /// Optional aura to apply: (AuraType, duration, magnitude)
    pub applies_aura: Option<(AuraType, f32, f32)>,
}

impl AbilityDefinition {
    /// Returns true if this is a healing ability
    pub fn is_heal(&self) -> bool {
        self.healing_max > 0.0
    }
    
    /// Returns true if this is a damage ability
    pub fn is_damage(&self) -> bool {
        self.damage_max > 0.0
    }
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
    mut combat_log: ResMut<CombatLog>,
    config: Res<MatchConfig>,
) {
    info!("Setting up Play Match scene with config: {:?}", *config);
    
    // Clear combat log for new match
    combat_log.clear();
    combat_log.log(CombatLogEventType::MatchEvent, "Match started!".to_string());

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

/// Render 2D health, resource, and cast bars above each living combatant's 3D position.
/// 
/// This system uses egui to draw bars in screen space, converting 3D world positions
/// to 2D screen coordinates. Displays:
/// - **Health bar** (always): Green/yellow/red based on HP percentage
/// - **Resource bar** (if applicable): Colored by resource type
///   - Mana (blue): Mages, Priests - regenerates slowly, starts full
///   - Energy (yellow): Rogues - regenerates rapidly, starts full
///   - Rage (red): Warriors - starts at 0, builds from attacks and taking damage
/// - **Cast bar** (when casting): Orange bar with spell name showing cast progress
pub fn render_health_bars(
    mut contexts: EguiContexts,
    combatants: Query<(&Combatant, &Transform, Option<&CastingState>)>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
) {
    let ctx = contexts.ctx_mut();
    
    let Ok((camera, camera_transform)) = camera_query.get_single() else {
        return;
    };

    egui::Area::new(egui::Id::new("health_bars"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .show(ctx, |ui| {
            for (combatant, transform, casting_state) in combatants.iter() {
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
                    let bar_spacing = 2.0; // Space between bars
                    let bar_pos = egui::pos2(
                        screen_pos.x - bar_width / 2.0,
                        screen_pos.y - bar_height / 2.0,
                    );

                    // Health bar background (dark gray)
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

                    // Health bar border
                    ui.painter().rect_stroke(
                        egui::Rect::from_min_size(bar_pos, egui::vec2(bar_width, bar_height)),
                        2.0,
                        egui::Stroke::new(1.0, egui::Color32::from_rgb(200, 200, 200)),
                    );
                    
                    // Resource bar (mana/energy/rage)
                    let mut next_bar_y_offset = bar_height + bar_spacing;
                    if combatant.max_mana > 0.0 {
                        let resource_percent = combatant.current_mana / combatant.max_mana;
                        let resource_bar_pos = egui::pos2(
                            bar_pos.x,
                            bar_pos.y + next_bar_y_offset,
                        );
                        let resource_bar_height = 4.0; // Slightly smaller than health bar
                        
                        // Determine resource color based on type
                        let (resource_color, border_color) = match combatant.resource_type {
                            ResourceType::Mana => (
                                egui::Color32::from_rgb(80, 150, 255),  // Blue
                                egui::Color32::from_rgb(150, 150, 200),
                            ),
                            ResourceType::Energy => (
                                egui::Color32::from_rgb(255, 255, 100), // Yellow
                                egui::Color32::from_rgb(200, 200, 150),
                            ),
                            ResourceType::Rage => (
                                egui::Color32::from_rgb(255, 80, 80),   // Red
                                egui::Color32::from_rgb(200, 150, 150),
                            ),
                        };
                        
                        // Resource bar background
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(resource_bar_pos, egui::vec2(bar_width, resource_bar_height)),
                            2.0,
                            egui::Color32::from_rgb(20, 20, 30),
                        );
                        
                        // Resource bar fill (colored by resource type)
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(
                                resource_bar_pos,
                                egui::vec2(bar_width * resource_percent, resource_bar_height),
                            ),
                            2.0,
                            resource_color,
                        );
                        
                        // Resource bar border
                        ui.painter().rect_stroke(
                            egui::Rect::from_min_size(resource_bar_pos, egui::vec2(bar_width, resource_bar_height)),
                            2.0,
                            egui::Stroke::new(1.0, border_color),
                        );
                        
                        next_bar_y_offset += resource_bar_height + bar_spacing;
                    }
                    
                    // Cast bar (only when actively casting)
                    if let Some(casting) = casting_state {
                        let ability_def = casting.ability.definition();
                        let cast_progress = 1.0 - (casting.time_remaining / ability_def.cast_time);
                        
                        let cast_bar_pos = egui::pos2(
                            bar_pos.x,
                            bar_pos.y + next_bar_y_offset,
                        );
                        let cast_bar_height = 8.0; // Slightly larger than other bars
                        let cast_bar_width = bar_width + 10.0; // Wider for better visibility
                        
                        // Adjust x position to keep it centered
                        let cast_bar_pos = egui::pos2(
                            cast_bar_pos.x - 5.0,
                            cast_bar_pos.y,
                        );
                        
                        // Cast bar background (darker)
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(cast_bar_pos, egui::vec2(cast_bar_width, cast_bar_height)),
                            2.0,
                            egui::Color32::from_rgb(15, 15, 20),
                        );
                        
                        // Cast bar fill (orange/yellow, WoW-style)
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(
                                cast_bar_pos,
                                egui::vec2(cast_bar_width * cast_progress, cast_bar_height),
                            ),
                            2.0,
                            egui::Color32::from_rgb(255, 180, 50), // Orange
                        );
                        
                        // Cast bar border
                        ui.painter().rect_stroke(
                            egui::Rect::from_min_size(cast_bar_pos, egui::vec2(cast_bar_width, cast_bar_height)),
                            2.0,
                            egui::Stroke::new(1.5, egui::Color32::from_rgb(255, 200, 100)),
                        );
                        
                        // Spell name text (centered on cast bar)
                        let text_pos = egui::pos2(
                            cast_bar_pos.x + cast_bar_width / 2.0,
                            cast_bar_pos.y + cast_bar_height / 2.0,
                        );
                        ui.painter().text(
                            text_pos,
                            egui::Align2::CENTER_CENTER,
                            ability_def.name,
                            egui::FontId::proportional(10.0),
                            egui::Color32::WHITE,
                        );
                    }
                }
            }
        });
}

/// Render the combat log in a scrollable panel.
/// 
/// Displays the most recent combat events in WoW-like fashion:
/// - Scrollable area on the left side of the screen
/// - Color-coded by event type (damage, healing, death)
/// - Auto-scrolls to show latest events
/// - Shows timestamp for each event
pub fn render_combat_log(
    mut contexts: EguiContexts,
    combat_log: Res<CombatLog>,
) {
    let ctx = contexts.ctx_mut();
    
    // Combat log panel on the left side
    egui::SidePanel::left("combat_log_panel")
        .default_width(350.0)
        .max_width(450.0)
        .min_width(250.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading(
                egui::RichText::new("Combat Log")
                    .size(20.0)
                    .color(egui::Color32::from_rgb(230, 204, 153))
            );
            
            ui.add_space(5.0);
            ui.separator();
            ui.add_space(5.0);
            
            // Scrollable area for log entries
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    // Show all combat log entries
                    for entry in &combat_log.entries {
                        // Color based on event type
                        let color = match entry.event_type {
                            CombatLogEventType::Damage => egui::Color32::from_rgb(255, 180, 180), // Light red
                            CombatLogEventType::Healing => egui::Color32::from_rgb(180, 255, 180), // Light green
                            CombatLogEventType::Death => egui::Color32::from_rgb(200, 100, 100), // Dark red
                            CombatLogEventType::MatchEvent => egui::Color32::from_rgb(200, 200, 100), // Yellow
                            _ => egui::Color32::from_rgb(200, 200, 200), // Gray
                        };
                        
                        // Format timestamp
                        let timestamp_str = format!("[{:>5.1}s]", entry.timestamp);
                        
                        ui.horizontal(|ui| {
                            // Timestamp in gray
                            ui.label(
                                egui::RichText::new(&timestamp_str)
                                    .size(12.0)
                                    .color(egui::Color32::from_rgb(150, 150, 150))
                            );
                            
                            // Event message in color
                            ui.label(
                                egui::RichText::new(&entry.message)
                                    .size(13.0)
                                    .color(color)
                            );
                        });
                    }
                });
        });
}

/// Update floating combat text - make it float upward and fade over time.
/// 
/// Each FCT floats upward at a constant speed and decreases its lifetime.
/// Expired FCT is not removed here (see `cleanup_expired_floating_text`).
pub fn update_floating_combat_text(
    time: Res<Time>,
    mut floating_texts: Query<&mut FloatingCombatText>,
) {
    let dt = time.delta_secs();
    
    for mut fct in floating_texts.iter_mut() {
        // Float upward
        fct.vertical_offset += 1.5 * dt; // Rise at 1.5 units/sec
        fct.world_position.y += 1.5 * dt;
        
        // Decrease lifetime
        fct.lifetime -= dt;
    }
}

/// Render floating combat text as 2D overlay.
/// 
/// Projects 3D world positions to 2D screen space and renders damage numbers.
/// Text fades out as lifetime decreases (alpha based on remaining lifetime).
pub fn render_floating_combat_text(
    mut contexts: EguiContexts,
    floating_texts: Query<&FloatingCombatText>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
) {
    let ctx = contexts.ctx_mut();
    
    let Ok((camera, camera_transform)) = camera_query.get_single() else {
        return;
    };
    
    egui::Area::new(egui::Id::new("floating_combat_text"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .show(ctx, |ui| {
            for fct in floating_texts.iter() {
                // Only render if still alive
                if fct.lifetime <= 0.0 {
                    continue;
                }
                
                // Project 3D position to 2D screen space
                if let Ok(screen_pos) = camera.world_to_viewport(camera_transform, fct.world_position) {
                    // Calculate alpha based on remaining lifetime
                    // Fade out in the last 0.5 seconds
                    let alpha = if fct.lifetime < 0.5 {
                        (fct.lifetime / 0.5 * 255.0) as u8
                    } else {
                        255
                    };
                    
                    // Apply alpha to color
                    let color_with_alpha = egui::Color32::from_rgba_unmultiplied(
                        fct.color.r(),
                        fct.color.g(),
                        fct.color.b(),
                        alpha,
                    );
                    
                    // Draw the damage number with outline for visibility
                    let font_id = egui::FontId::proportional(24.0);
                    
                    // Draw black outline (offset in 4 directions for better visibility)
                    for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                        ui.painter().text(
                            egui::pos2(screen_pos.x + dx, screen_pos.y + dy),
                            egui::Align2::CENTER_CENTER,
                            &fct.text,
                            font_id.clone(),
                            egui::Color32::from_rgba_unmultiplied(0, 0, 0, alpha),
                        );
                    }
                    
                    // Draw main text
                    ui.painter().text(
                        egui::pos2(screen_pos.x, screen_pos.y),
                        egui::Align2::CENTER_CENTER,
                        &fct.text,
                        font_id,
                        color_with_alpha,
                    );
                }
            }
        });
}

/// Cleanup expired floating combat text.
/// 
/// Despawns FCT entities when their lifetime reaches zero.
pub fn cleanup_expired_floating_text(
    mut commands: Commands,
    floating_texts: Query<(Entity, &FloatingCombatText)>,
) {
    for (entity, fct) in floating_texts.iter() {
        if fct.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
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
    // Build list of all alive combatants with their info (excluding stealthed enemies)
    let alive_combatants: Vec<(Entity, u8, Vec3, bool)> = combatants
        .iter()
        .filter(|(_, c, _)| c.is_alive())
        .map(|(entity, c, transform)| (entity, c.team, transform.translation, c.stealthed))
        .collect();

    // For each combatant, ensure they have a valid target
    for (_entity, mut combatant, transform) in combatants.iter_mut() {
        if !combatant.is_alive() {
            combatant.target = None;
            continue;
        }

        // Check if current target is still valid (alive, on enemy team, and not stealthed)
        let target_valid = combatant.target.and_then(|target_entity| {
            alive_combatants
                .iter()
                .find(|(e, _, _, _)| *e == target_entity)
                .filter(|(_, team, _, stealthed)| *team != combatant.team && !stealthed)
        }).is_some();

        // If no valid target, find nearest enemy (excluding stealthed)
        if !target_valid {
            let my_pos = transform.translation;
            let nearest_enemy = alive_combatants
                .iter()
                .filter(|(_, team, _, stealthed)| *team != combatant.team && !stealthed)
                .min_by(|(_, _, pos_a, _), (_, _, pos_b, _)| {
                    let dist_a = my_pos.distance(*pos_a);
                    let dist_b = my_pos.distance(*pos_b);
                    dist_a.partial_cmp(&dist_b).unwrap()
                });

            combatant.target = nearest_enemy.map(|(entity, _, _, _)| *entity);
        }
    }
}

/// Movement system: Move combatants towards their targets if out of range.
/// 
/// Combatants will:
/// - Move towards their target if they have one and are out of melee range
/// - Stop moving when within attack range
/// - Rotate to face their target
/// - **WoW Mechanic**: Cannot move while casting (checked via `CastingState`)
/// - Movement speed modified by auras (e.g., Frostbolt's slow)
/// 
/// This creates the WoW-like behavior where melee combatants chase each other around the arena.
pub fn move_to_target(
    time: Res<Time>,
    mut combatants: Query<(Entity, &mut Transform, &Combatant, Option<&ActiveAuras>, Option<&CastingState>)>,
) {
    let dt = time.delta_secs();
    
    // Build a snapshot of all combatant positions for lookups
    let positions: std::collections::HashMap<Entity, Vec3> = combatants
        .iter()
        .map(|(entity, transform, _, _, _)| (entity, transform.translation))
        .collect();
    
    // Move each combatant towards their target if needed
    for (_entity, mut transform, combatant, auras, casting_state) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // Cannot move while casting (WoW mechanic)
        if casting_state.is_some() {
            continue;
        }
        
        // Get target position
        let Some(target_entity) = combatant.target else {
            continue;
        };
        
        let Some(&target_pos) = positions.get(&target_entity) else {
            continue;
        };
        
        let my_pos = transform.translation;
        let distance = my_pos.distance(target_pos);
        
        // If out of range, move towards target
        if distance > STOP_DISTANCE {
            // Calculate direction to target (only in XZ plane, keep Y constant)
            let direction = Vec3::new(
                target_pos.x - my_pos.x,
                0.0, // Don't move vertically
                target_pos.z - my_pos.z,
            ).normalize_or_zero();
            
            if direction != Vec3::ZERO {
                // Calculate effective movement speed (base * aura modifiers)
                let mut movement_speed = combatant.base_movement_speed;
                if let Some(auras) = auras {
                    for aura in &auras.auras {
                        if aura.effect_type == AuraType::MovementSpeedSlow {
                            movement_speed *= aura.magnitude;
                        }
                    }
                }
                
                // Move towards target
                let move_distance = movement_speed * dt;
                transform.translation += direction * move_distance;
                
                // Rotate to face target
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }
        }
    }
}

/// Auto-attack system: Process attacks based on attack speed timers.
/// 
/// Each combatant has an attack timer that counts up. When it reaches
/// the attack interval (1.0 / attack_speed), they check if they're in
/// range and attack their target.
/// 
/// **Range Check**: Only melee attacks for now, must be within MELEE_RANGE.
/// **WoW Mechanic**: Cannot auto-attack while casting (checked via `CastingState`).
/// 
/// Damage is applied immediately and stats are updated for both attacker and target.
/// All attacks are logged to the combat log for display.
pub fn combat_auto_attack(
    time: Res<Time>,
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut combatants: Query<(Entity, &Transform, &mut Combatant, Option<&CastingState>)>,
) {
    let dt = time.delta_secs();
    
    // Update match time in combat log
    combat_log.match_time += dt;
    
    // Build a snapshot of positions for range checks
    let positions: std::collections::HashMap<Entity, Vec3> = combatants
        .iter()
        .map(|(entity, transform, _, _)| (entity, transform.translation))
        .collect();
    
    // Build a snapshot of combatant info for logging
    let combatant_info: std::collections::HashMap<Entity, (u8, match_config::CharacterClass)> = combatants
        .iter()
        .map(|(entity, _, combatant, _)| (entity, (combatant.team, combatant.class)))
        .collect();
    
    // Collect attacks that will happen this frame (attacker, target, damage)
    let mut attacks = Vec::new();
    
    // Track damage per target for batching floating combat text
    let mut damage_per_target: std::collections::HashMap<Entity, f32> = std::collections::HashMap::new();
    
    for (attacker_entity, transform, mut combatant, casting_state) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // WoW Mechanic: Cannot auto-attack while casting
        if casting_state.is_some() {
            continue;
        }
        
        // WoW Mechanic: Cannot auto-attack while stealthed (Rogues must use abilities)
        if combatant.stealthed {
            continue;
        }

        // Update attack timer
        combatant.attack_timer += dt;

        // Check if ready to attack and has a target
        let attack_interval = 1.0 / combatant.attack_speed;
        if combatant.attack_timer >= attack_interval {
            if let Some(target_entity) = combatant.target {
                // Check if target is in range before attacking
                if let Some(&target_pos) = positions.get(&target_entity) {
                    let my_pos = transform.translation;
                    
                    if combatant.in_attack_range(my_pos, target_pos) {
                        // Calculate total damage (base + bonus from Heroic Strike, etc.)
                        let total_damage = combatant.attack_damage + combatant.next_attack_bonus_damage;
                        let has_bonus = combatant.next_attack_bonus_damage > 0.0;
                        
                        attacks.push((attacker_entity, target_entity, total_damage, has_bonus));
                        combatant.attack_timer = 0.0;
                        
                        // Consume the bonus damage after queueing the attack
                        combatant.next_attack_bonus_damage = 0.0;
                        
                        // Break stealth on auto-attack
                        if combatant.stealthed {
                            combatant.stealthed = false;
                            info!(
                                "Team {} {} breaks stealth with auto-attack!",
                                combatant.team,
                                combatant.class.name()
                            );
                        }
                        
                        // Warriors generate Rage from auto-attacks
                        if combatant.resource_type == ResourceType::Rage {
                            let rage_gain = 10.0; // Gain 10 rage per auto-attack
                            combatant.current_mana = (combatant.current_mana + rage_gain).min(combatant.max_mana);
                        }
                    }
                    // If not in range, timer keeps building up so they attack immediately when in range
                }
            }
        }
    }

    // Apply damage to targets and track damage dealt
    let mut damage_dealt_updates: Vec<(Entity, f32)> = Vec::new();
    
    for (attacker_entity, target_entity, damage, has_bonus) in attacks {
        if let Ok((_, _, mut target, _)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                let actual_damage = damage.min(target.current_health);
                target.current_health = (target.current_health - damage).max(0.0);
                target.damage_taken += actual_damage;
                
                // Warriors generate Rage from taking damage
                if target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15; // Gain 15% of damage taken as Rage
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }
                
                // Batch damage for floating combat text (sum all damage to same target)
                *damage_per_target.entry(target_entity).or_insert(0.0) += actual_damage;
                
                // Collect attacker damage for later update
                damage_dealt_updates.push((attacker_entity, actual_damage));
                
                // Log the attack with position data
                if let (Some(&(attacker_team, attacker_class)), Some(&(target_team, target_class))) = 
                    (combatant_info.get(&attacker_entity), combatant_info.get(&target_entity)) {
                    let attack_name = if has_bonus {
                        "Heroic Strike" // Enhanced auto-attack
                    } else {
                        "Auto Attack"
                    };
                    let message = format!(
                        "Team {} {}'s {} hits Team {} {} for {:.0} damage",
                        attacker_team,
                        attacker_class.name(),
                        attack_name,
                        target_team,
                        target_class.name(),
                        actual_damage
                    );
                    
                    // Get positions for logging
                    if let (Some(&attacker_pos), Some(&target_pos)) = 
                        (positions.get(&attacker_entity), positions.get(&target_entity)) {
                        let distance = attacker_pos.distance(target_pos);
                        combat_log.log_with_position(
                            CombatLogEventType::Damage,
                            message,
                            PositionData {
                                entities: vec![
                                    format!("Team {} {} (attacker)", attacker_team, attacker_class.name()),
                                    format!("Team {} {} (target)", target_team, target_class.name()),
                                ],
                                positions: vec![
                                    (attacker_pos.x, attacker_pos.y, attacker_pos.z),
                                    (target_pos.x, target_pos.y, target_pos.z),
                                ],
                                distance: Some(distance),
                            },
                        );
                    } else {
                        combat_log.log(CombatLogEventType::Damage, message);
                    }
                }
                
                if !target.is_alive() {
                    // Log death
                    if let Some(&(target_team, target_class)) = combatant_info.get(&target_entity) {
                        let message = format!(
                            "Team {} {} has been eliminated",
                            target_team,
                            target_class.name()
                        );
                        combat_log.log(CombatLogEventType::Death, message);
                    }
                }
            }
        }
    }
    
    // Spawn floating combat text for each target that took damage (batched)
    for (target_entity, total_damage) in damage_per_target {
        if let Some(&target_pos) = positions.get(&target_entity) {
            // Spawn floating text slightly above the combatant
            let text_position = target_pos + Vec3::new(0.0, 2.0, 0.0);
            
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position,
                    text: format!("{:.0}", total_damage),
                    color: egui::Color32::WHITE, // White for auto-attacks
                    lifetime: 1.5, // Display for 1.5 seconds
                    vertical_offset: 0.0,
                },
                PlayMatchEntity,
            ));
        }
    }
    
    // Update attacker damage dealt stats
    for (attacker_entity, damage) in damage_dealt_updates {
        if let Ok((_, _, mut attacker, _)) = combatants.get_mut(attacker_entity) {
            attacker.damage_dealt += damage;
        }
    }
}

/// Resource regeneration system: Regenerate mana for all combatants.
/// 
/// Each combatant with mana regeneration gains mana per second up to their max.
pub fn regenerate_resources(
    time: Res<Time>,
    mut combatants: Query<&mut Combatant>,
) {
    let dt = time.delta_secs();
    
    for mut combatant in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // Regenerate mana
        if combatant.mana_regen > 0.0 {
            combatant.current_mana = (combatant.current_mana + combatant.mana_regen * dt).min(combatant.max_mana);
        }
    }
}

/// Ability decision system: AI decides when to cast abilities.
/// 
/// - **Mages**: Cast Frostbolt on enemies when in range and have mana
/// - **Priests**: Cast Flash Heal on lowest HP ally (including self) when needed
/// - **Rogues**: Use Ambush from stealth for high burst damage
/// 
/// Future: More complex decision trees, cooldowns, priorities, etc.
pub fn decide_abilities(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut combatants: Query<(Entity, &mut Combatant, &Transform), Without<CastingState>>,
) {
    // Build position and info maps from all combatants
    let positions: std::collections::HashMap<Entity, Vec3> = combatants
        .iter()
        .map(|(entity, _, transform)| (entity, transform.translation))
        .collect();
    
    let combatant_info: std::collections::HashMap<Entity, (u8, match_config::CharacterClass, f32, f32)> = combatants
        .iter()
        .map(|(entity, combatant, _)| {
            (entity, (combatant.team, combatant.class, combatant.current_health, combatant.max_health))
        })
        .collect();
    
    // Queue for Ambush attacks (attacker, target, damage, team, class)
    let mut ambush_attacks: Vec<(Entity, Entity, f32, u8, match_config::CharacterClass)> = Vec::new();
    
    for (entity, mut combatant, transform) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        let my_pos = transform.translation;
        
        // Mages cast Frostbolt on enemies
        if combatant.class == match_config::CharacterClass::Mage {
            // Check if we have an enemy target
            let Some(target_entity) = combatant.target else {
                continue;
            };
            
            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };
            
            // Try to cast Frostbolt
            let ability = AbilityType::Frostbolt;
            if ability.can_cast(&combatant, target_pos, my_pos) {
                let def = ability.definition();
                
                // Start casting
                commands.entity(entity).insert(CastingState {
                    ability,
                    time_remaining: def.cast_time,
                    target: Some(target_entity),
                });
                
                info!(
                    "Team {} {} starts casting {} on enemy",
                    combatant.team,
                    combatant.class.name(),
                    def.name
                );
            }
        }
        // Priests cast Flash Heal on injured allies
        else if combatant.class == match_config::CharacterClass::Priest {
            // Find the lowest HP ally (including self)
            let mut lowest_hp_ally: Option<(Entity, f32, Vec3)> = None;
            
            for (ally_entity, &(ally_team, _ally_class, ally_hp, ally_max_hp)) in combatant_info.iter() {
                // Must be same team and alive
                if ally_team != combatant.team {
                    continue;
                }
                
                // Only heal if damaged (below 90% health)
                let hp_percent = ally_hp / ally_max_hp;
                if hp_percent >= 0.9 {
                    continue;
                }
                
                // Get position
                let Some(&ally_pos) = positions.get(ally_entity) else {
                    continue;
                };
                
                // Track lowest HP ally
                match lowest_hp_ally {
                    None => lowest_hp_ally = Some((*ally_entity, hp_percent, ally_pos)),
                    Some((_, lowest_percent, _)) if hp_percent < lowest_percent => {
                        lowest_hp_ally = Some((*ally_entity, hp_percent, ally_pos));
                    }
                    _ => {}
                }
            }
            
            // Cast heal on lowest HP ally if found
            if let Some((heal_target, _, target_pos)) = lowest_hp_ally {
                let ability = AbilityType::FlashHeal;
                if ability.can_cast(&combatant, target_pos, my_pos) {
                    let def = ability.definition();
                    
                    // Start casting
                    commands.entity(entity).insert(CastingState {
                        ability,
                        time_remaining: def.cast_time,
                        target: Some(heal_target),
                    });
                    
                    info!(
                        "Team {} {} starts casting {} on ally",
                        combatant.team,
                        combatant.class.name(),
                        def.name
                    );
                }
            }
        }
        
        // Warriors use Heroic Strike (instant cast, enhances next auto-attack)
        if combatant.class == match_config::CharacterClass::Warrior {
            // Check if we have an enemy target and already have bonus damage queued
            let Some(target_entity) = combatant.target else {
                continue;
            };
            
            // Don't queue another Heroic Strike if one is already pending
            if combatant.next_attack_bonus_damage > 0.0 {
                continue;
            }
            
            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };
            
            // Try to use Heroic Strike if we have enough rage and target is in melee range
            let ability = AbilityType::HeroicStrike;
            if ability.can_cast(&combatant, target_pos, my_pos) {
                let def = ability.definition();
                
                // Since it's instant, apply the effect immediately
                // Consume rage
                combatant.current_mana -= def.mana_cost;
                
                // Set bonus damage for next auto-attack (50% of base attack damage)
                let bonus_damage = combatant.attack_damage * 0.5;
                combatant.next_attack_bonus_damage = bonus_damage;
                
                info!(
                    "Team {} {} uses {} (next attack +{:.0} damage)",
                    combatant.team,
                    combatant.class.name(),
                    def.name,
                    bonus_damage
                );
            }
        }
        
        // Rogues use Ambush from stealth (instant ability, high damage)
        if combatant.class == match_config::CharacterClass::Rogue && combatant.stealthed {
            // Check if we have an enemy target
            let Some(target_entity) = combatant.target else {
                continue;
            };
            
            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };
            
            // Try to use Ambush if we have enough energy and target is in melee range
            let ability = AbilityType::Ambush;
            if ability.can_cast(&combatant, target_pos, my_pos) {
                let def = ability.definition();
                
                // Consume energy
                combatant.current_mana -= def.mana_cost;
                
                // Break stealth immediately
                combatant.stealthed = false;
                
                // Calculate damage (random between min and max)
                let damage_range = def.damage_max - def.damage_min;
                let damage = def.damage_min + (rand::random::<f32>() * damage_range);
                
                // Queue the Ambush attack to be applied after the loop
                ambush_attacks.push((entity, target_entity, damage, combatant.team, combatant.class));
                
                info!(
                    "Team {} {} uses {} from stealth!",
                    combatant.team,
                    combatant.class.name(),
                    def.name
                );
            }
        }
    }
    
    // Process queued Ambush attacks
    for (attacker_entity, target_entity, damage, attacker_team, attacker_class) in ambush_attacks {
        let mut actual_damage = 0.0;
        let mut target_team = 0;
        let mut target_class = match_config::CharacterClass::Warrior; // Default, will be overwritten
        
        if let Ok((_, mut target, target_transform)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                actual_damage = damage.min(target.current_health);
                target.current_health = (target.current_health - damage).max(0.0);
                target.damage_taken += actual_damage;
                target_team = target.team;
                target_class = target.class;
                
                // Warriors generate Rage from taking damage
                if target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15;
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }
                
                info!(
                    "Team {} {}'s Ambush hits Team {} {} for {:.0} damage!",
                    attacker_team,
                    attacker_class.name(),
                    target_team,
                    target_class.name(),
                    actual_damage
                );
                
                // Spawn floating combat text (yellow for abilities)
                let text_position = target_transform.translation + Vec3::new(0.0, 2.0, 0.0);
                commands.spawn((
                    FloatingCombatText {
                        world_position: text_position,
                        text: format!("{:.0}", actual_damage),
                        color: egui::Color32::from_rgb(255, 255, 0), // Yellow for abilities
                        lifetime: 1.5,
                        vertical_offset: 0.0,
                    },
                    PlayMatchEntity,
                ));
                
                // Log the Ambush attack with position data
                let message = format!(
                    "Team {} {}'s Ambush hits Team {} {} for {:.0} damage",
                    attacker_team,
                    attacker_class.name(),
                    target_team,
                    target_class.name(),
                    actual_damage
                );
                
                if let (Some(&attacker_pos), Some(&target_pos)) = 
                    (positions.get(&attacker_entity), positions.get(&target_entity)) {
                    let distance = attacker_pos.distance(target_pos);
                    combat_log.log_with_position(
                        CombatLogEventType::Damage,
                        message,
                        PositionData {
                            entities: vec![
                                format!("Team {} {} (attacker)", attacker_team, attacker_class.name()),
                                format!("Team {} {} (target)", target_team, target_class.name()),
                            ],
                            positions: vec![
                                (attacker_pos.x, attacker_pos.y, attacker_pos.z),
                                (target_pos.x, target_pos.y, target_pos.z),
                            ],
                            distance: Some(distance),
                        },
                    );
                } else {
                    combat_log.log(CombatLogEventType::Damage, message);
                }
            }
        }
        
        // Update attacker's damage dealt
        if let Ok((_, mut attacker, _)) = combatants.get_mut(attacker_entity) {
            attacker.damage_dealt += actual_damage;
        }
    }
}

/// Casting system: Process active casts, complete them when time is up.
/// 
/// Reduces cast timers each frame. When a cast completes:
/// 1. Consume mana
/// 2. Deal damage (for damage spells) or heal (for healing spells)
/// 3. Apply auras (if applicable)
/// 4. Spawn floating combat text (yellow for damage, green for healing)
/// 5. Log to combat log with position data
pub fn process_casting(
    time: Res<Time>,
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut combatants: Query<(Entity, &Transform, &mut Combatant, Option<&mut CastingState>)>,
) {
    let dt = time.delta_secs();
    
    // Track completed casts
    let mut completed_casts = Vec::new();
    
    // First pass: update cast timers and collect completed casts
    for (caster_entity, caster_transform, mut caster, casting_state) in combatants.iter_mut() {
        let Some(mut casting) = casting_state else {
            continue;
        };
        
        if !caster.is_alive() {
            // Cancel cast if caster dies
            commands.entity(caster_entity).remove::<CastingState>();
            continue;
        }
        
        // Tick down cast time
        casting.time_remaining -= dt;
        
        // Check if cast completed
        if casting.time_remaining <= 0.0 {
            let ability = casting.ability;
            let def = ability.definition();
            let target_entity = casting.target;
            
            // Consume mana
            caster.current_mana -= def.mana_cost;
            
            // Store cast info for processing
            completed_casts.push((
                caster_entity,
                caster.team,
                caster.class,
                caster_transform.translation,
                ability,
                target_entity,
            ));
            
            // Remove casting state
            commands.entity(caster_entity).remove::<CastingState>();
        }
    }
    
    // Track damage_dealt updates for casters (to apply after processing all casts)
    let mut caster_damage_updates: Vec<(Entity, f32)> = Vec::new();
    // Track healing_done updates for healers (to apply after processing all casts)
    let mut caster_healing_updates: Vec<(Entity, f32)> = Vec::new();
    // Track casters who should have stealth broken (offensive abilities)
    let mut break_stealth: Vec<Entity> = Vec::new();
    
    // Process completed casts
    for (caster_entity, caster_team, caster_class, caster_pos, ability, target_entity) in completed_casts {
        let def = ability.definition();
        
        // Get target
        let Some(target_entity) = target_entity else {
            continue;
        };
        
        // Check if this is self-targeting (e.g., priest healing themselves)
        let is_self_target = target_entity == caster_entity;
        
        // Get target combatant
        let Ok((_, target_transform, mut target, _)) = combatants.get_mut(target_entity) else {
            continue;
        };
        
        if !target.is_alive() {
            continue;
        }
        
        let target_pos = target_transform.translation;
        let distance = caster_pos.distance(target_pos);
        let text_position = target_transform.translation + Vec3::new(0.0, 2.0, 0.0);
        
        // Handle damage spells
        if def.is_damage() {
            // Calculate damage (random between min and max)
            let damage_range = def.damage_max - def.damage_min;
            let damage = def.damage_min + (rand::random::<f32>() * damage_range);
            
            let actual_damage = damage.min(target.current_health);
            target.current_health = (target.current_health - damage).max(0.0);
            target.damage_taken += actual_damage;
            
            // Warriors generate Rage from taking damage
            if target.resource_type == ResourceType::Rage {
                let rage_gain = actual_damage * 0.15; // Gain 15% of damage taken as Rage
                target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
            }
            
            // Break stealth on offensive ability use
            break_stealth.push(caster_entity);
            
            // Track damage dealt for caster (update later to avoid double borrow)
            if is_self_target {
                // Self-damage: target IS caster, so update now
                target.damage_dealt += actual_damage;
            } else {
                // Different target: collect for later update
                caster_damage_updates.push((caster_entity, actual_damage));
            }
            
            // Spawn floating combat text (yellow for damage abilities)
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position,
                    text: format!("{:.0}", actual_damage),
                    color: egui::Color32::from_rgb(255, 255, 0), // Yellow for abilities
                    lifetime: 1.5,
                    vertical_offset: 0.0,
                },
                PlayMatchEntity,
            ));
            
            // Log the damage
            let message = format!(
                "Team {} {}'s {} hits Team {} {} for {:.0} damage",
                caster_team,
                caster_class.name(),
                def.name,
                target.team,
                target.class.name(),
                actual_damage
            );
            combat_log.log_with_position(
                CombatLogEventType::Damage,
                message,
                PositionData {
                    entities: vec![
                        format!("Team {} {} (caster)", caster_team, caster_class.name()),
                        format!("Team {} {} (target)", target.team, target.class.name()),
                    ],
                    positions: vec![
                        (caster_pos.x, caster_pos.y, caster_pos.z),
                        (target_pos.x, target_pos.y, target_pos.z),
                    ],
                    distance: Some(distance),
                },
            );
        }
        // Handle healing spells
        else if def.is_heal() {
            // Calculate healing (random between min and max)
            let healing_range = def.healing_max - def.healing_min;
            let healing = def.healing_min + (rand::random::<f32>() * healing_range);
            
            // Apply healing (don't overheal)
            let actual_healing = healing.min(target.max_health - target.current_health);
            target.current_health = (target.current_health + healing).min(target.max_health);
            
            // Track healing done for healer (update later to avoid double borrow)
            if is_self_target {
                // Self-healing: target IS caster, so update now
                target.healing_done += actual_healing;
            } else {
                // Different target: collect for later update
                caster_healing_updates.push((caster_entity, actual_healing));
            }
            
            // Spawn floating combat text (green for healing)
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position,
                    text: format!("+{:.0}", actual_healing),
                    color: egui::Color32::from_rgb(100, 255, 100), // Green for healing
                    lifetime: 1.5,
                    vertical_offset: 0.0,
                },
                PlayMatchEntity,
            ));
            
            // Log the healing
            let message = format!(
                "Team {} {}'s {} heals Team {} {} for {:.0}",
                caster_team,
                caster_class.name(),
                def.name,
                target.team,
                target.class.name(),
                actual_healing
            );
            combat_log.log_with_position(
                CombatLogEventType::Healing,
                message,
                PositionData {
                    entities: vec![
                        format!("Team {} {} (caster)", caster_team, caster_class.name()),
                        format!("Team {} {} (target)", target.team, target.class.name()),
                    ],
                    positions: vec![
                        (caster_pos.x, caster_pos.y, caster_pos.z),
                        (target_pos.x, target_pos.y, target_pos.z),
                    ],
                    distance: Some(distance),
                },
            );
        }
        
        // Apply aura if applicable (store for later application)
        if let Some((aura_type, duration, magnitude)) = def.applies_aura {
            // We'll apply auras in a separate pass to avoid borrow issues
            commands.spawn((
                AuraPending {
                    target: target_entity,
                    aura: Aura {
                        effect_type: aura_type.clone(),
                        duration,
                        magnitude,
                    },
                },
                PlayMatchEntity,
            ));
            
            info!(
                "Queued {:?} aura for Team {} {} (magnitude: {}, duration: {}s)",
                aura_type,
                target.team,
                target.class.name(),
                magnitude,
                duration
            );
        }
        
        // Check for death
        if !target.is_alive() {
            let message = format!(
                "Team {} {} has been eliminated",
                target.team,
                target.class.name()
            );
            combat_log.log(CombatLogEventType::Death, message);
        }
    }
    
    // Apply collected caster damage updates
    for (caster_entity, damage) in caster_damage_updates {
        if let Ok((_, _, mut caster, _)) = combatants.get_mut(caster_entity) {
            caster.damage_dealt += damage;
        }
    }
    
    // Apply collected healer healing updates
    for (healer_entity, healing) in caster_healing_updates {
        if let Ok((_, _, mut healer, _)) = combatants.get_mut(healer_entity) {
            healer.healing_done += healing;
        }
    }
    
    // Break stealth for casters who used offensive abilities
    for caster_entity in break_stealth {
        if let Ok((_, _, mut caster, _)) = combatants.get_mut(caster_entity) {
            if caster.stealthed {
                caster.stealthed = false;
                info!(
                    "Team {} {} breaks stealth!",
                    caster.team,
                    caster.class.name()
                );
            }
        }
    }
}

/// Aura update system: Tick down aura durations and remove expired auras.
/// 
/// This runs each frame to:
/// 1. Decrease duration of all active auras
/// 2. Remove auras with duration <= 0
/// 3. Remove ActiveAuras component if no auras remain
pub fn update_auras(
    time: Res<Time>,
    mut commands: Commands,
    mut combatants: Query<(Entity, &mut ActiveAuras)>,
) {
    let dt = time.delta_secs();
    
    for (entity, mut auras) in combatants.iter_mut() {
        // Tick down all aura durations
        for aura in auras.auras.iter_mut() {
            aura.duration -= dt;
        }
        
        // Remove expired auras
        auras.auras.retain(|aura| aura.duration > 0.0);
        
        // Remove component if no auras remain
        if auras.auras.is_empty() {
            commands.entity(entity).remove::<ActiveAuras>();
        }
    }
}

/// Apply pending auras to targets.
/// 
/// This system runs after casting completes and applies any queued auras
/// to their targets. It handles both new auras and stacking existing auras.
pub fn apply_pending_auras(
    mut commands: Commands,
    pending_auras: Query<(Entity, &AuraPending)>,
    mut combatants: Query<&mut ActiveAuras>,
) {
    for (pending_entity, pending) in pending_auras.iter() {
        // Try to get existing auras on target
        if let Ok(mut active_auras) = combatants.get_mut(pending.target) {
            // Add to existing auras
            active_auras.auras.push(pending.aura.clone());
        } else {
            // No existing auras, insert new component
            commands.entity(pending.target).insert(ActiveAuras {
                auras: vec![pending.aura.clone()],
            });
        }
        
        // Remove the pending aura entity
        commands.entity(pending_entity).despawn();
    }
}

/// Check if the match has ended (one or both teams eliminated).
/// 
/// When the match ends:
/// 1. Determine winner (or draw if both teams die simultaneously)
/// 2. Collect final stats for all combatants
/// 3. Save combat log to file for debugging
/// 4. Insert `MatchResults` resource for the Results scene
/// 5. Transition to Results state
pub fn check_match_end(
    combatants: Query<(&Combatant, &Transform)>,
    config: Res<MatchConfig>,
    combat_log: Res<CombatLog>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<GameState>>,
) {
    let team1_alive = combatants.iter().any(|(c, _)| c.team == 1 && c.is_alive());
    let team2_alive = combatants.iter().any(|(c, _)| c.team == 2 && c.is_alive());

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
        
        for (combatant, transform) in combatants.iter() {
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
        }
        
        // Save combat log to file for debugging
        let match_metadata = MatchMetadata {
            arena_name: config.map.name().to_string(),
            winner,
            team1: team1_metadata,
            team2: team2_metadata,
        };
        
        match combat_log.save_to_file(&match_metadata) {
            Ok(filename) => {
                info!("Combat log saved to: {}", filename);
            }
            Err(e) => {
                error!("Failed to save combat log: {}", e);
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

