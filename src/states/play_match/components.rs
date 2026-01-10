//! Component Definitions for Play Match
//!
//! This module contains all ECS components, resources, and data structures
//! used during the match simulation.

use bevy::prelude::*;
use bevy_egui::egui;
use super::match_config;
use super::abilities::{AbilityDefinition, AbilityType, ScalingStat};

// Re-export constants from parent module
use super::{MELEE_RANGE, WAND_RANGE};

// ============================================================================
// Resources & Camera
// ============================================================================

/// Controls the speed of combat simulation
#[derive(Resource)]
pub struct SimulationSpeed {
    pub multiplier: f32,
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

// ============================================================================
// Marker Components
// ============================================================================

/// Marker component for all entities spawned in the Play Match scene.
/// Used for cleanup when exiting the scene.
#[derive(Component)]
pub struct PlayMatchEntity;

/// Marker component for the arena camera
#[derive(Component)]
pub struct ArenaCamera;

/// Component marking a combatant as celebrating (for bounce animation)
#[derive(Component)]
pub struct Celebrating {
    /// Time offset for staggered bounce timing
    pub bounce_offset: f32,
}

/// Component tracking floating combat text pattern state for deterministic spreading
#[derive(Component)]
pub struct FloatingTextState {
    /// Pattern index for next text spawn: 0 (center), 1 (right), 2 (left), cycles
    pub next_pattern_index: u8,
}

/// Component for gate bars that lower when countdown ends
#[derive(Component)]
pub struct GateBar {
    /// Which team this gate belongs to (1 or 2)
    pub team: u8,
    /// Initial height of the gate bar
    pub initial_height: f32,
}

/// Component for speech bubbles that appear when abilities are used
#[derive(Component)]
pub struct SpeechBubble {
    /// Entity this speech bubble is attached to
    pub owner: Entity,
    /// Text to display
    pub text: String,
    /// Time until this bubble disappears (in seconds)
    pub lifetime: f32,
}

// ============================================================================
// Enums
// ============================================================================

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

/// Types of aura effects.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AuraType {
    /// Reduces movement speed by a percentage (magnitude = multiplier, e.g., 0.7 = 30% slow)
    MovementSpeedSlow,
    /// Prevents movement (rooted in place) - magnitude unused
    Root,
    /// Prevents all actions (movement, casting, auto-attacks, abilities) - magnitude unused
    Stun,
    /// Increases maximum health by a flat amount (magnitude = HP bonus)
    MaxHealthIncrease,
    /// Deals damage periodically (magnitude = damage per tick, tick_interval determines frequency)
    DamageOverTime,
    /// Spell school lockout - prevents casting spells of a specific school
    /// The magnitude field stores the locked school as f32 (cast from SpellSchool enum)
    SpellSchoolLockout,
    /// Reduces healing received by a percentage (magnitude = multiplier, e.g., 0.65 = 35% reduction)
    HealingReduction,
    /// Fear - target runs around randomly, unable to act. Breaks on damage.
    Fear,
    // Future: Silence, Healing-over-time, Attack Power buffs, etc.
}

// ============================================================================
// Combat Components
// ============================================================================

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
    /// Attack Power - scales physical damage abilities and auto-attacks
    pub attack_power: f32,
    /// Spell Power - scales magical damage and healing abilities
    pub spell_power: f32,
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
    /// Original color before stealth visual effects were applied
    pub original_color: Color,
    /// Cooldown timers for abilities (ability type -> remaining cooldown in seconds)
    pub ability_cooldowns: std::collections::HashMap<AbilityType, f32>,
    /// Global cooldown timer - prevents ability spam (1.5s standard GCD in WoW)
    pub global_cooldown: f32,
    /// When > 0, combatant will move away from enemies (kiting). Decrements over time.
    pub kiting_timer: f32,
}

impl Combatant {
    /// Create a new combatant with class-specific stats.
    pub fn new(team: u8, class: match_config::CharacterClass) -> Self {
        // Class-specific stats (resource_type, health, max_resource, resource_regen, starting_resource, damage, attack speed, attack_power, spell_power, movement speed)
        let (resource_type, max_health, max_resource, resource_regen, starting_resource, attack_damage, attack_speed, attack_power, spell_power, movement_speed) = match class {
            // Warriors: High HP, physical damage, scales with Attack Power
            match_config::CharacterClass::Warrior => (ResourceType::Rage, 200.0, 100.0, 0.0, 0.0, 12.0, 1.0, 30.0, 0.0, 5.0),
            // Mages: Low HP, magical damage (wand), scales with Spell Power
            match_config::CharacterClass::Mage => (ResourceType::Mana, 150.0, 200.0, 10.0, 200.0, 10.0, 0.7, 0.0, 50.0, 4.5),
            // Rogues: Medium HP, physical burst damage, scales with Attack Power
            match_config::CharacterClass::Rogue => (ResourceType::Energy, 175.0, 100.0, 20.0, 100.0, 10.0, 1.3, 35.0, 0.0, 6.0),
            // Priests: Medium HP, healing & wand damage, scales with Spell Power
            match_config::CharacterClass::Priest => (ResourceType::Mana, 150.0, 150.0, 8.0, 150.0, 6.0, 0.8, 0.0, 40.0, 5.0),
            // Warlocks: Medium HP, shadow damage (wand), scales with Spell Power, DoT focused
            match_config::CharacterClass::Warlock => (ResourceType::Mana, 160.0, 180.0, 9.0, 180.0, 8.0, 0.7, 0.0, 45.0, 4.5),
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
            attack_power,
            spell_power,
            base_movement_speed: movement_speed,
            target: None,
            damage_dealt: 0.0,
            damage_taken: 0.0,
            healing_done: 0.0,
            next_attack_bonus_damage: 0.0,
            stealthed,
            original_color: Color::WHITE, // Will be set correctly when spawning the visual mesh
            ability_cooldowns: std::collections::HashMap::new(),
            global_cooldown: 0.0,
            kiting_timer: 0.0,
        }
    }
    
    /// Check if this combatant is alive (health > 0).
    pub fn is_alive(&self) -> bool {
        self.current_health > 0.0
    }
    
    /// Check if this combatant is in range to attack the target position.
    /// Mages, Priests, and Warlocks use wands (ranged), Warriors and Rogues use melee weapons.
    pub fn in_attack_range(&self, my_position: Vec3, target_position: Vec3) -> bool {
        let distance = my_position.distance(target_position);
        match self.class {
            match_config::CharacterClass::Mage | match_config::CharacterClass::Priest | match_config::CharacterClass::Warlock => {
                distance <= WAND_RANGE
            }
            match_config::CharacterClass::Warrior | match_config::CharacterClass::Rogue => {
                distance <= MELEE_RANGE
            }
        }
    }
    
    /// Calculate damage for an ability based on character stats.
    /// Formula: Base Damage + (Scaling Stat × Coefficient)
    pub fn calculate_ability_damage(&self, ability_def: &AbilityDefinition) -> f32 {
        // Calculate base damage (random between min and max)
        let damage_range = ability_def.damage_base_max - ability_def.damage_base_min;
        let base_damage = ability_def.damage_base_min + (rand::random::<f32>() * damage_range);
        
        // Add stat scaling
        let stat_value = match ability_def.damage_scales_with {
            ScalingStat::AttackPower => self.attack_power,
            ScalingStat::SpellPower => self.spell_power,
            ScalingStat::None => 0.0,
        };
        
        base_damage + (stat_value * ability_def.damage_coefficient)
    }
    
    /// Calculate healing for an ability based on character stats.
    /// Formula: Base Healing + (Spell Power × Coefficient)
    pub fn calculate_ability_healing(&self, ability_def: &AbilityDefinition) -> f32 {
        // Calculate base healing (random between min and max)
        let healing_range = ability_def.healing_base_max - ability_def.healing_base_min;
        let base_healing = ability_def.healing_base_min + (rand::random::<f32>() * healing_range);
        
        // Add spell power scaling (healing always scales with spell power in WoW)
        base_healing + (self.spell_power * ability_def.healing_coefficient)
    }
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
    /// Whether this cast was interrupted (for visual feedback)
    pub interrupted: bool,
    /// Time remaining to show interrupted state (before removing CastingState)
    pub interrupted_display_time: f32,
}

/// Component tracking an active Charge (Warrior gap closer).
#[derive(Component)]
pub struct ChargingState {
    /// Target entity being charged toward
    pub target: Entity,
}

/// Component tracking active auras/debuffs on a combatant.
#[derive(Component, Default)]
pub struct ActiveAuras {
    pub auras: Vec<Aura>,
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
    /// Damage threshold before the aura breaks (0.0 = never breaks on damage)
    pub break_on_damage_threshold: f32,
    /// Accumulated damage taken while this aura is active
    pub accumulated_damage: f32,
    /// For DoT effects: how often damage is applied (in seconds)
    pub tick_interval: f32,
    /// For DoT effects: time remaining until next tick
    pub time_until_next_tick: f32,
    /// For DoT effects: who applied this aura (for damage attribution)
    pub caster: Option<Entity>,
    /// Name of the ability that created this aura (for logging)
    pub ability_name: String,
    /// For Fear: current run direction (x, z normalized)
    pub fear_direction: (f32, f32),
    /// For Fear: time until direction change
    pub fear_direction_timer: f32,
}

/// Temporary component for pending auras to be applied.
/// Used to avoid borrow checker issues when applying auras during casting.
#[derive(Component)]
pub struct AuraPending {
    pub target: Entity,
    pub aura: Aura,
}

/// Component for pending interrupt attempts.
/// Spawned as a temporary entity to interrupt a target's cast.
#[derive(Component)]
pub struct InterruptPending {
    /// The entity that cast the interrupt
    pub caster: Entity,
    /// The target entity to interrupt
    pub target: Entity,
    /// The interrupt ability used (Pummel, Kick, etc.)
    pub ability: AbilityType,
    /// Duration of the spell school lockout (in seconds)
    pub lockout_duration: f32,
}

/// Component tracking damage taken this frame for aura breaking purposes.
#[derive(Component, Default)]
pub struct DamageTakenThisFrame {
    pub amount: f32,
}

/// Component for spell projectiles that travel from caster to target.
/// When the projectile reaches its target, damage/effects are applied.
#[derive(Component)]
pub struct Projectile {
    /// The entity that cast this projectile
    pub caster: Entity,
    /// The target entity this projectile is traveling towards
    pub target: Entity,
    /// The ability this projectile represents (for damage calculation)
    pub ability: AbilityType,
    /// Travel speed in units per second
    pub speed: f32,
    /// Team of the caster (for combat log)
    pub caster_team: u8,
    /// Class of the caster (for combat log)
    pub caster_class: match_config::CharacterClass,
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

/// Visual effect for spell impacts (Mind Blast, etc.)
/// Displays as an expanding sphere that fades out
#[derive(Component)]
pub struct SpellImpactEffect {
    /// World position where the effect should appear
    pub position: Vec3,
    /// Time remaining before effect disappears (in seconds)
    pub lifetime: f32,
    /// Initial lifetime for calculating fade/scale
    pub initial_lifetime: f32,
    /// Initial scale of the sphere
    pub initial_scale: f32,
    /// Final scale of the sphere (expands to this)
    pub final_scale: f32,
}

