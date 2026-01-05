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
//! - Simple arena floor (60x60 plane)
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

/// Arena size (80x80 plane centered at origin, includes starting areas)
const ARENA_HALF_SIZE: f32 = 40.0;

// ============================================================================
// Resources
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
        }
    }
}

// ============================================================================
// Components
// ============================================================================

/// Marker component for all entities spawned in the Play Match scene.
/// Used for cleanup when exiting the scene.
#[derive(Component)]
pub struct PlayMatchEntity;

/// Marker component for the arena camera
#[derive(Component)]
pub struct ArenaCamera;

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

/// Component marking a combatant as celebrating (for bounce animation)
#[derive(Component)]
pub struct Celebrating {
    /// Time offset for staggered bounce timing
    pub bounce_offset: f32,
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
            match_config::CharacterClass::Warrior => (ResourceType::Rage, 150.0, 100.0, 0.0, 0.0, 12.0, 1.0, 30.0, 0.0, 5.0),
            // Mages: Low HP, magical damage, scales with Spell Power
            match_config::CharacterClass::Mage => (ResourceType::Mana, 80.0, 200.0, 10.0, 200.0, 20.0, 0.7, 0.0, 50.0, 4.5),
            // Rogues: Medium HP, physical burst damage, scales with Attack Power
            match_config::CharacterClass::Rogue => (ResourceType::Energy, 100.0, 100.0, 20.0, 100.0, 10.0, 1.3, 35.0, 0.0, 6.0),
            // Priests: Medium HP, healing & damage, scales with Spell Power
            match_config::CharacterClass::Priest => (ResourceType::Mana, 90.0, 150.0, 8.0, 150.0, 8.0, 0.8, 0.0, 40.0, 5.0),
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
    pub fn in_attack_range(&self, my_position: Vec3, target_position: Vec3) -> bool {
        my_position.distance(target_position) <= MELEE_RANGE
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
}

/// Spell schools - determines which spells share lockouts when interrupted.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SpellSchool {
    /// Physical abilities (melee attacks, weapon strikes)
    Physical,
    /// Frost magic (Frostbolt, Frost Nova)
    Frost,
    /// Holy magic (Flash Heal, Power Word: Fortitude)
    Holy,
    /// Shadow magic (Mind Blast)
    Shadow,
    /// No spell school (can't be locked out)
    None,
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
    // Future: Silence, Healing-over-time, Attack Power buffs, etc.
}

/// Enum representing available abilities.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AbilityType {
    Frostbolt,
    FlashHeal,
    HeroicStrike,
    Ambush,
    FrostNova,
    MindBlast,
    SinisterStrike,
    Charge,
    KidneyShot,
    PowerWordFortitude,
    Rend,
    Pummel,    // Warrior interrupt
    Kick,      // Rogue interrupt
    // Future: Fireball, Backstab, etc.
}

impl AbilityType {
    /// Get ability definition (cast time, range, cost, etc.)
    pub fn definition(&self) -> AbilityDefinition {
        match self {
            AbilityType::Frostbolt => AbilityDefinition {
                name: "Frostbolt",
                cast_time: 1.5, // Reduced from 2.5s to see projectiles more often
                range: 30.0,
                mana_cost: 20.0,
                cooldown: 0.0,
                damage_base_min: 10.0,
                damage_base_max: 15.0,
                damage_coefficient: 0.8, // 80% of Spell Power added to damage
                damage_scales_with: ScalingStat::SpellPower,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: Some((AuraType::MovementSpeedSlow, 5.0, 0.7, 0.0)), // 30% slow for 5s, doesn't break on damage
                projectile_speed: Some(20.0), // Travels at 20 units/second
                spell_school: SpellSchool::Frost,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::FlashHeal => AbilityDefinition {
                name: "Flash Heal",
                cast_time: 1.5,
                range: 40.0, // Longer range than Frostbolt
                mana_cost: 25.0,
                cooldown: 0.0,
                damage_base_min: 0.0,
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 15.0,
                healing_base_max: 20.0,
                healing_coefficient: 0.75, // 75% of Spell Power added to healing
                applies_aura: None,
                projectile_speed: None, // Instant effect, no projectile
                spell_school: SpellSchool::Holy,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::HeroicStrike => AbilityDefinition {
                name: "Heroic Strike",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE,
                mana_cost: 15.0, // Costs 15 Rage
                cooldown: 0.0, // No cooldown
                damage_base_min: 0.0, // No direct damage - enhances next auto-attack
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None,
                projectile_speed: None, // Melee ability, no projectile
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::Ambush => AbilityDefinition {
                name: "Ambush",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE,
                mana_cost: 60.0, // High energy cost
                cooldown: 0.0,
                damage_base_min: 20.0, // High burst damage
                damage_base_max: 30.0,
                damage_coefficient: 1.2, // 120% of Attack Power - very high!
                damage_scales_with: ScalingStat::AttackPower,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None,
                projectile_speed: None, // Melee ability, no projectile
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::FrostNova => AbilityDefinition {
                name: "Frost Nova",
                cast_time: 0.0, // Instant cast
                range: 10.0, // AOE range - affects all enemies within this distance
                mana_cost: 30.0,
                cooldown: 25.0, // 25 second cooldown
                damage_base_min: 5.0, // Small AOE damage
                damage_base_max: 10.0,
                damage_coefficient: 0.2, // 20% of Spell Power
                damage_scales_with: ScalingStat::SpellPower,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: Some((AuraType::Root, 6.0, 1.0, 35.0)), // Root for 6s, breaks on 35+ damage
                projectile_speed: None, // Instant AOE, no projectile
                spell_school: SpellSchool::Frost,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::MindBlast => AbilityDefinition {
                name: "Mind Blast",
                cast_time: 1.5, // Same as Frostbolt
                range: 30.0, // Ranged spell
                mana_cost: 25.0,
                cooldown: 8.0, // Short cooldown for consistent damage
                damage_base_min: 15.0, // Good damage
                damage_base_max: 20.0,
                damage_coefficient: 0.85, // 85% of Spell Power
                damage_scales_with: ScalingStat::SpellPower,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None, // Pure damage, no debuff
                projectile_speed: None, // Instant effect (shadow magic)
                spell_school: SpellSchool::Shadow,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::SinisterStrike => AbilityDefinition {
                name: "Sinister Strike",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE, // Melee ability
                mana_cost: 40.0, // 40 energy cost
                cooldown: 0.0, // No inherent cooldown, uses GCD
                damage_base_min: 5.0, // Base weapon damage
                damage_base_max: 10.0,
                damage_coefficient: 0.5, // 50% of Attack Power
                damage_scales_with: ScalingStat::AttackPower,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None,
                projectile_speed: None, // Instant melee strike
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::Charge => AbilityDefinition {
                name: "Charge",
                cast_time: 0.0, // Instant cast
                range: 25.0, // Max 25 units (minimum 8 units checked separately)
                mana_cost: 0.0, // No rage cost (generates rage in WoW, but we'll keep it simple)
                cooldown: 15.0, // Medium cooldown - can't spam it
                damage_base_min: 0.0, // No damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None,
                projectile_speed: None, // Movement ability, not a projectile
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::KidneyShot => AbilityDefinition {
                name: "Kidney Shot",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE, // Melee ability
                mana_cost: 60.0, // 60 energy cost (significant)
                cooldown: 30.0, // Long cooldown - powerful CC
                damage_base_min: 0.0, // No damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Stun for 6 seconds, doesn't break on damage (break_threshold = 0.0)
                applies_aura: Some((AuraType::Stun, 6.0, 1.0, 0.0)),
                projectile_speed: None, // Instant melee strike
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::PowerWordFortitude => AbilityDefinition {
                name: "Power Word: Fortitude",
                cast_time: 0.0, // Instant cast
                range: 40.0, // Same range as Flash Heal
                mana_cost: 30.0, // Moderate mana cost
                cooldown: 0.0, // No cooldown - can buff entire team quickly
                damage_base_min: 0.0, // No damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0, // Not a heal
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Increase max HP by 30 for 600 seconds (10 minutes, effectively permanent)
                // Magnitude = 30 HP, duration = 600s, no damage breaking
                applies_aura: Some((AuraType::MaxHealthIncrease, 600.0, 30.0, 0.0)),
                projectile_speed: None, // Instant buff
                spell_school: SpellSchool::Holy,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::Rend => AbilityDefinition {
                name: "Rend",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE, // Melee ability
                mana_cost: 10.0, // 10 rage cost
                cooldown: 0.0, // No cooldown, but can't be reapplied if target already has it
                damage_base_min: 0.0, // No direct damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                // Apply DoT: 15 second duration, 8 damage per tick (ticks every 3 seconds = 5 ticks total)
                // Magnitude = damage per tick, tick_interval stored separately in Aura
                applies_aura: Some((AuraType::DamageOverTime, 15.0, 8.0, 0.0)),
                projectile_speed: None, // Instant melee application
                spell_school: SpellSchool::Physical,
                is_interrupt: false,
                lockout_duration: 0.0,
            },
            AbilityType::Pummel => AbilityDefinition {
                name: "Pummel",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE, // Melee ability
                mana_cost: 10.0, // 10 rage cost
                cooldown: 12.0, // Medium cooldown
                damage_base_min: 0.0, // No damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None, // Interrupt is handled specially
                projectile_speed: None, // Instant melee interrupt
                spell_school: SpellSchool::Physical,
                is_interrupt: true,
                lockout_duration: 4.0, // 4 second lockout
            },
            AbilityType::Kick => AbilityDefinition {
                name: "Kick",
                cast_time: 0.0, // Instant cast
                range: MELEE_RANGE, // Melee ability
                mana_cost: 25.0, // 25 energy cost
                cooldown: 12.0, // Medium cooldown
                damage_base_min: 0.0, // No damage
                damage_base_max: 0.0,
                damage_coefficient: 0.0,
                damage_scales_with: ScalingStat::None,
                healing_base_min: 0.0,
                healing_base_max: 0.0,
                healing_coefficient: 0.0,
                applies_aura: None, // Interrupt is handled specially
                projectile_speed: None, // Instant melee interrupt
                spell_school: SpellSchool::Physical,
                is_interrupt: true,
                lockout_duration: 4.0, // 4 second lockout
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
/// What stat an ability scales with for damage/healing
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ScalingStat {
    /// Scales with Attack Power (physical abilities and auto-attacks)
    AttackPower,
    /// Scales with Spell Power (magical abilities and healing)
    SpellPower,
    /// Doesn't scale with any stat (CC abilities, utility)
    None,
}

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
    /// Base minimum damage (before stat scaling)
    pub damage_base_min: f32,
    /// Base maximum damage (before stat scaling)
    pub damage_base_max: f32,
    /// Coefficient: how much damage per point of Attack Power or Spell Power
    /// Formula: Damage = Base + (Stat × Coefficient)
    pub damage_coefficient: f32,
    /// What stat this ability's damage scales with
    pub damage_scales_with: ScalingStat,
    /// Base minimum healing (before stat scaling)
    pub healing_base_min: f32,
    /// Base maximum healing (before stat scaling)
    pub healing_base_max: f32,
    /// Coefficient: how much healing per point of Spell Power
    /// Formula: Healing = Base + (Spell Power × Coefficient)
    pub healing_coefficient: f32,
    /// Optional aura to apply: (AuraType, duration, magnitude, break_on_damage_threshold)
    /// break_on_damage_threshold: 0.0 = never breaks on damage
    pub applies_aura: Option<(AuraType, f32, f32, f32)>,
    /// Projectile travel speed in units/second (None = instant effect, no projectile)
    pub projectile_speed: Option<f32>,
    /// Spell school (determines lockout when interrupted)
    pub spell_school: SpellSchool,
    /// Whether this ability interrupts the target's casting
    pub is_interrupt: bool,
    /// Lockout duration in seconds (for interrupt abilities)
    pub lockout_duration: f32,
}

impl AbilityDefinition {
    /// Returns true if this is a damage ability
    pub fn is_damage(&self) -> bool {
        self.damage_base_max > 0.0 || self.damage_coefficient > 0.0
    }
    
    /// Returns true if this is a healing ability
    pub fn is_heal(&self) -> bool {
        self.healing_base_max > 0.0 || self.healing_coefficient > 0.0
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
        Transform::from_xyz(0.0, 40.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
        ArenaCamera,
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
    
    // Initialize simulation speed control
    commands.insert_resource(SimulationSpeed { multiplier: 1.0 });
    
    // Initialize camera controller
    commands.insert_resource(CameraController::default());
    
    // Initialize match countdown (10 seconds before gates open)
    commands.insert_resource(MatchCountdown::default());

    // Spawn arena floor - 80x80 unit plane (includes starting areas)
    let floor_size = 80.0;
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

    // Spawn Team 1 combatants (left side of arena, in starting pen)
    // Teams start further back (-35/+35) and will move forward when gates open
    let team1_spawn_x = -35.0;
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

    // Spawn Team 2 combatants (right side of arena, in starting pen)
    let team2_spawn_x = 35.0;
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
        // Enable alpha mode for stealth transparency
        alpha_mode: bevy::prelude::AlphaMode::Blend,
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

/// Handle camera input for mode switching, zoom, rotation, and drag
pub fn handle_camera_input(
    mut camera_controller: ResMut<CameraController>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut mouse_wheel: EventReader<bevy::input::mouse::MouseWheel>,
    mut cursor_moved: EventReader<bevy::window::CursorMoved>,
    combatants: Query<Entity, With<Combatant>>,
) {
    // Cycle camera modes with TAB
    if keyboard.just_pressed(KeyCode::Tab) {
        camera_controller.mode = match camera_controller.mode {
            CameraMode::FollowCenter => {
                // Find first alive combatant to follow
                if let Some(entity) = combatants.iter().next() {
                    CameraMode::FollowCombatant(entity)
                } else {
                    CameraMode::FollowCenter
                }
            }
            CameraMode::FollowCombatant(current_entity) => {
                // Cycle to next combatant
                let mut found_current = false;
                let mut next_entity = None;
                
                for entity in combatants.iter() {
                    if found_current {
                        next_entity = Some(entity);
                        break;
                    }
                    if entity == current_entity {
                        found_current = true;
                    }
                }
                
                // If we found a next entity, use it. Otherwise, go to manual or back to center
                if let Some(entity) = next_entity {
                    CameraMode::FollowCombatant(entity)
                } else {
                    CameraMode::Manual
                }
            }
            CameraMode::Manual => CameraMode::FollowCenter,
        };
    }
    
    // Reset camera to center with 'C' key
    if keyboard.just_pressed(KeyCode::KeyC) {
        camera_controller.mode = CameraMode::FollowCenter;
        camera_controller.zoom_distance = 60.0;
        camera_controller.pitch = 38.7f32.to_radians();
        camera_controller.yaw = 0.0;
    }
    
    // Handle mouse wheel for zoom
    for event in mouse_wheel.read() {
        let zoom_delta = event.y * 3.0; // Zoom speed
        camera_controller.zoom_distance = (camera_controller.zoom_distance - zoom_delta).clamp(20.0, 150.0);
    }
    
    // Handle mouse drag for rotation (middle mouse button)
    if mouse_button.just_pressed(MouseButton::Middle) {
        camera_controller.is_dragging = true;
        
        // When starting manual mode, we need to preserve the current target
        // We'll update manual_target in the update_camera_position system
    }
    
    if mouse_button.just_released(MouseButton::Middle) {
        camera_controller.is_dragging = false;
        camera_controller.last_mouse_pos = None;
    }
    
    if camera_controller.is_dragging {
        for event in cursor_moved.read() {
            if let Some(last_pos) = camera_controller.last_mouse_pos {
                let delta = event.position - last_pos;
                
                // Update yaw and pitch based on drag
                camera_controller.yaw -= delta.x * 0.005; // Horizontal rotation
                camera_controller.pitch = (camera_controller.pitch - delta.y * 0.005).clamp(0.1, 1.5); // Vertical rotation, clamped
            }
            camera_controller.last_mouse_pos = Some(event.position);
        }
    } else {
        // Update last mouse pos even when not dragging, so first drag frame isn't a huge jump
        for event in cursor_moved.read() {
            camera_controller.last_mouse_pos = Some(event.position);
        }
    }
}

/// Update camera position and rotation based on controller state
pub fn update_camera_position(
    mut camera_controller: ResMut<CameraController>,
    mut camera_query: Query<&mut Transform, With<ArenaCamera>>,
    combatants: Query<(Entity, &Transform, &Combatant), Without<ArenaCamera>>,
) {
    let Ok(mut camera_transform) = camera_query.get_single_mut() else {
        return;
    };
    
    // If user just started dragging, switch to manual mode and preserve current target
    if camera_controller.is_dragging && camera_controller.mode != CameraMode::Manual {
        // Calculate current target before switching to manual
        let current_target = match camera_controller.mode {
            CameraMode::FollowCenter => {
                let alive_combatants: Vec<Vec3> = combatants
                    .iter()
                    .filter(|(_, _, c)| c.is_alive())
                    .map(|(_, t, _)| t.translation)
                    .collect();
                if alive_combatants.is_empty() {
                    Vec3::ZERO
                } else {
                    let sum: Vec3 = alive_combatants.iter().sum();
                    sum / alive_combatants.len() as f32
                }
            }
            CameraMode::FollowCombatant(target_entity) => {
                combatants
                    .iter()
                    .find(|(e, _, _)| *e == target_entity)
                    .map(|(_, t, _)| t.translation)
                    .unwrap_or(Vec3::ZERO)
            }
            CameraMode::Manual => camera_controller.manual_target,
        };
        
        camera_controller.manual_target = current_target;
        camera_controller.mode = CameraMode::Manual;
    }
    
    // Determine the target look-at point based on camera mode
    let target_point = match camera_controller.mode {
        CameraMode::FollowCenter => {
            // Calculate center of all alive combatants
            let alive_combatants: Vec<Vec3> = combatants
                .iter()
                .filter(|(_, _, c)| c.is_alive())
                .map(|(_, t, _)| t.translation)
                .collect();
            
            if alive_combatants.is_empty() {
                Vec3::ZERO
            } else {
                let sum: Vec3 = alive_combatants.iter().sum();
                sum / alive_combatants.len() as f32
            }
        }
        CameraMode::FollowCombatant(target_entity) => {
            // Follow specific combatant
            combatants
                .iter()
                .find(|(e, _, _)| *e == target_entity)
                .map(|(_, t, _)| t.translation)
                .unwrap_or(Vec3::ZERO)
        }
        CameraMode::Manual => {
            // Use manual target (preserved when entering manual mode)
            camera_controller.manual_target
        }
    };
    
    // Calculate camera position based on spherical coordinates
    let x = target_point.x + camera_controller.zoom_distance * camera_controller.pitch.sin() * camera_controller.yaw.sin();
    let y = target_point.y + camera_controller.zoom_distance * camera_controller.pitch.cos();
    let z = target_point.z + camera_controller.zoom_distance * camera_controller.pitch.sin() * camera_controller.yaw.cos();
    
    camera_transform.translation = Vec3::new(x, y, z);
    camera_transform.look_at(target_point, Vec3::Y);
}

/// Render camera controls help overlay
pub fn render_camera_controls(
    mut contexts: EguiContexts,
    camera_controller: Res<CameraController>,
) {
    let ctx = contexts.ctx_mut();
    
    // Position in bottom-left corner
    egui::Window::new("Camera Controls")
        .fixed_pos(egui::pos2(10.0, ctx.screen_rect().height() - 160.0))
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style())
            .fill(egui::Color32::from_black_alpha(150))) // Semi-transparent
        .show(ctx, |ui| {
            ui.set_width(250.0);
            
            // Current mode
            let mode_text = match camera_controller.mode {
                CameraMode::FollowCenter => "Center",
                CameraMode::FollowCombatant(_) => "Follow Combatant",
                CameraMode::Manual => "Manual",
            };
            
            ui.label(
                egui::RichText::new(format!("Mode: {}", mode_text))
                    .size(12.0)
                    .color(egui::Color32::from_rgb(100, 200, 255))
                    .strong()
            );
            
            ui.add_space(5.0);
            
            // Controls
            ui.label(
                egui::RichText::new("TAB - Cycle camera mode")
                    .size(11.0)
                    .color(egui::Color32::from_rgb(200, 200, 200))
            );
            ui.label(
                egui::RichText::new("C - Reset to center")
                    .size(11.0)
                    .color(egui::Color32::from_rgb(200, 200, 200))
            );
            ui.label(
                egui::RichText::new("Mouse Wheel - Zoom")
                    .size(11.0)
                    .color(egui::Color32::from_rgb(200, 200, 200))
            );
            ui.label(
                egui::RichText::new("Middle Mouse - Rotate")
                    .size(11.0)
                    .color(egui::Color32::from_rgb(200, 200, 200))
            );
        });
}

/// Cleanup system: Despawns all Play Match entities when exiting the state.
pub fn cleanup_play_match(
    mut commands: Commands,
    query: Query<Entity, With<PlayMatchEntity>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
    
    // Remove resources
    commands.remove_resource::<AmbientLight>();
    commands.remove_resource::<SimulationSpeed>();
    commands.remove_resource::<MatchCountdown>();
    // Remove optional resources (may not exist if match didn't finish)
    commands.remove_resource::<VictoryCelebration>();
}

// ============================================================================
// Update & Input Systems
// ============================================================================

/// Countdown system: Manage pre-combat countdown and gate opening.
/// 
/// During countdown (10 seconds):
/// - Mana is restored to 100% every second (encourages pre-buffing)
/// - Combatants can cast buffs but cannot move or attack
/// - Countdown timer ticks down
/// 
/// When countdown reaches 0:
/// - Gates open (sets gates_opened flag)
/// - Combat begins normally
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

/// Handle time control keyboard shortcuts and apply time multiplier to simulation.
/// 
/// **Keyboard Shortcuts:**
/// - `Space`: Pause/Unpause
/// - `1`: 0.5x speed
/// - `2`: 1x speed (normal)
/// - `3`: 2x speed
/// - `4`: 3x speed
pub fn handle_time_controls(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut sim_speed: ResMut<SimulationSpeed>,
    mut time: ResMut<Time<Virtual>>,
) {
    let mut speed_changed = false;
    let old_multiplier = sim_speed.multiplier;
    
    // Space toggles pause
    if keyboard.just_pressed(KeyCode::Space) {
        if sim_speed.is_paused() {
            sim_speed.multiplier = 1.0; // Resume at normal speed
        } else {
            sim_speed.multiplier = 0.0; // Pause
        }
        speed_changed = true;
    }
    
    // Number keys set specific speeds
    if keyboard.just_pressed(KeyCode::Digit1) {
        sim_speed.multiplier = 0.5;
        speed_changed = true;
    }
    if keyboard.just_pressed(KeyCode::Digit2) {
        sim_speed.multiplier = 1.0;
        speed_changed = true;
    }
    if keyboard.just_pressed(KeyCode::Digit3) {
        sim_speed.multiplier = 2.0;
        speed_changed = true;
    }
    if keyboard.just_pressed(KeyCode::Digit4) {
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

/// Render time control UI panel in the top-right corner.
/// 
/// Shows current speed and clickable buttons for speed control.
pub fn render_time_controls(
    mut contexts: EguiContexts,
    mut sim_speed: ResMut<SimulationSpeed>,
    mut time: ResMut<Time<Virtual>>,
) {
    let ctx = contexts.ctx_mut();
    
    // Position in top-right corner
    let screen_width = ctx.screen_rect().width();
    let panel_width = 180.0;
    
    egui::Window::new("Time Controls")
        .fixed_pos(egui::pos2(screen_width - panel_width - 10.0, 10.0))
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style())
            .fill(egui::Color32::from_black_alpha(200))) // Semi-transparent
        .show(ctx, |ui| {
            ui.set_width(panel_width);
            
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Speed:")
                        .size(14.0)
                        .color(egui::Color32::from_rgb(200, 200, 200))
                );
                
                let speed_text = if sim_speed.is_paused() {
                    "PAUSED"
                } else {
                    match sim_speed.multiplier {
                        x if (x - 0.5).abs() < 0.01 => "0.5x",
                        x if (x - 1.0).abs() < 0.01 => "1x",
                        x if (x - 2.0).abs() < 0.01 => "2x",
                        x if (x - 3.0).abs() < 0.01 => "3x",
                        _ => "??",
                    }
                };
                
                ui.label(
                    egui::RichText::new(speed_text)
                        .size(14.0)
                        .color(if sim_speed.is_paused() {
                            egui::Color32::from_rgb(255, 100, 100)
                        } else {
                            egui::Color32::from_rgb(100, 255, 100)
                        })
                        .strong()
                );
            });
            
            ui.add_space(5.0);
            
            ui.horizontal(|ui| {
                // Pause button
                let pause_btn = egui::Button::new(
                    egui::RichText::new(if sim_speed.is_paused() { "▶" } else { "⏸" })
                        .size(16.0)
                ).min_size(egui::vec2(35.0, 30.0));
                
                if ui.add(pause_btn).clicked() {
                    if sim_speed.is_paused() {
                        sim_speed.multiplier = 1.0;
                    } else {
                        sim_speed.multiplier = 0.0;
                    }
                    time.set_relative_speed(sim_speed.multiplier);
                }
                
                // Speed buttons
                for &speed in &[0.5, 1.0, 2.0, 3.0] {
                    let is_active = !sim_speed.is_paused() && (sim_speed.multiplier - speed).abs() < 0.01;
                    let label = if speed == 0.5 { "½x" } else { &format!("{}x", speed as u8) };
                    
                    let btn = egui::Button::new(
                        egui::RichText::new(label).size(12.0)
                    )
                    .min_size(egui::vec2(32.0, 30.0))
                    .fill(if is_active {
                        egui::Color32::from_rgb(60, 80, 120)
                    } else {
                        egui::Color32::from_rgb(40, 40, 50)
                    });
                    
                    if ui.add(btn).clicked() {
                        sim_speed.multiplier = speed;
                        time.set_relative_speed(sim_speed.multiplier);
                    }
                }
            });
            
            ui.add_space(3.0);
            
            // Keyboard shortcuts hint
            ui.label(
                egui::RichText::new("Space=Pause 1-4=Speed")
                    .size(10.0)
                    .color(egui::Color32::from_rgb(120, 120, 120))
            );
        });
}

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
/// Render the countdown timer during the pre-combat phase.
/// 
/// Displays a large centered countdown timer showing remaining seconds until gates open.
/// Also shows "Prepare for battle!" message to indicate pre-buffing phase.
pub fn render_countdown(
    mut contexts: EguiContexts,
    countdown: Res<MatchCountdown>,
) {
    // Only show countdown if gates haven't opened yet
    if countdown.gates_opened {
        return;
    }
    
    let ctx = contexts.ctx_mut();
    
    // Display countdown in center of screen
    egui::Area::new(egui::Id::new("match_countdown"))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, -50.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                // Large countdown number
                let seconds_remaining = countdown.time_remaining.ceil() as i32;
                ui.label(
                    egui::RichText::new(format!("{}", seconds_remaining))
                        .size(120.0)
                        .color(egui::Color32::from_rgb(255, 215, 0)) // Gold color
                        .strong()
                );
                
                ui.add_space(10.0);
                
                // "Prepare for battle!" message
                ui.label(
                    egui::RichText::new("Prepare for battle!")
                        .size(32.0)
                        .color(egui::Color32::from_rgb(230, 230, 230))
                );
                
                ui.add_space(5.0);
                
                // Hint about buffing
                ui.label(
                    egui::RichText::new("Apply buffs to your team!")
                        .size(18.0)
                        .color(egui::Color32::from_rgb(180, 180, 180))
                        .italics()
                );
            });
        });
}

/// - **Resource bar** (if applicable): Colored by resource type
///   - Mana (blue): Mages, Priests - regenerates slowly, starts full
///   - Energy (yellow): Rogues - regenerates rapidly, starts full
///   - Rage (red): Warriors - starts at 0, builds from attacks and taking damage
/// - **Cast bar** (when casting): Orange bar with spell name showing cast progress
pub fn render_health_bars(
    mut contexts: EguiContexts,
    combatants: Query<(&Combatant, &Transform, Option<&CastingState>, Option<&ActiveAuras>)>,
    camera_query: Query<(&Camera, &GlobalTransform)>,
) {
    let ctx = contexts.ctx_mut();
    
    let Ok((camera, camera_transform)) = camera_query.get_single() else {
        return;
    };

    egui::Area::new(egui::Id::new("health_bars"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .show(ctx, |ui| {
            for (combatant, transform, casting_state, active_auras) in combatants.iter() {
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
                    
                    // STEALTH indicator (if stealthed)
                    // Status indicators above health bar
                    let mut status_offset = -12.0; // Starting position above health bar
                    
                    // STEALTH indicator (if stealthed)
                    if combatant.stealthed {
                        let stealth_text = "STEALTH";
                        let stealth_font = egui::FontId::monospace(9.0);
                        let stealth_galley = ui.fonts(|f| f.layout_no_wrap(
                            stealth_text.to_string(),
                            stealth_font,
                            egui::Color32::from_rgb(150, 100, 200), // Purple tint
                        ));
                        let stealth_pos = egui::pos2(
                            bar_pos.x + (bar_width - stealth_galley.size().x) / 2.0,
                            bar_pos.y + status_offset,
                        );
                        ui.painter().galley(stealth_pos, stealth_galley, egui::Color32::from_rgb(150, 100, 200));
                        status_offset -= 10.0; // Move next label up
                    }
                    
                    // Status effect indicators (if has auras)
                    if let Some(auras) = active_auras {
                        // STUNNED indicator (if has Stun aura)
                        if auras.auras.iter().any(|a| a.effect_type == AuraType::Stun) {
                            let stun_text = "STUNNED";
                            let stun_font = egui::FontId::monospace(9.0);
                            let stun_galley = ui.fonts(|f| f.layout_no_wrap(
                                stun_text.to_string(),
                                stun_font,
                                egui::Color32::from_rgb(255, 100, 100), // Red
                            ));
                            let stun_pos = egui::pos2(
                                bar_pos.x + (bar_width - stun_galley.size().x) / 2.0,
                                bar_pos.y + status_offset,
                            );
                            ui.painter().galley(stun_pos, stun_galley, egui::Color32::from_rgb(255, 100, 100));
                            status_offset -= 10.0; // Move next label up
                        }
                        
                        // ROOTED indicator (if has Root aura)
                        if auras.auras.iter().any(|a| a.effect_type == AuraType::Root) {
                            let root_text = "ROOTED";
                            let root_font = egui::FontId::monospace(9.0);
                            let root_galley = ui.fonts(|f| f.layout_no_wrap(
                                root_text.to_string(),
                                root_font,
                                egui::Color32::from_rgb(100, 180, 255), // Ice blue
                            ));
                            let root_pos = egui::pos2(
                                bar_pos.x + (bar_width - root_galley.size().x) / 2.0,
                                bar_pos.y + status_offset,
                            );
                            ui.painter().galley(root_pos, root_galley, egui::Color32::from_rgb(100, 180, 255));
                        }
                    }

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
                        
                        // Interrupted casts show in RED
                        if casting.interrupted {
                            // Red background for interrupted
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(cast_bar_pos, egui::vec2(cast_bar_width, cast_bar_height)),
                                2.0,
                                egui::Color32::from_rgb(150, 20, 20), // Dark red
                            );
                            
                            // Red border
                            ui.painter().rect_stroke(
                                egui::Rect::from_min_size(cast_bar_pos, egui::vec2(cast_bar_width, cast_bar_height)),
                                2.0,
                                egui::Stroke::new(1.5, egui::Color32::from_rgb(220, 50, 50)),
                            );
                            
                            // "INTERRUPTED" text in white
                            let text_pos = egui::pos2(
                                cast_bar_pos.x + cast_bar_width / 2.0,
                                cast_bar_pos.y + cast_bar_height / 2.0,
                            );
                            ui.painter().text(
                                text_pos,
                                egui::Align2::CENTER_CENTER,
                                "INTERRUPTED",
                                egui::FontId::proportional(10.0),
                                egui::Color32::WHITE,
                            );
                        } else {
                            // Normal cast bar
                            let cast_progress = 1.0 - (casting.time_remaining / ability_def.cast_time);
                            
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
    
    // Combat log panel on the left side - semi-transparent to reduce obstruction
    egui::SidePanel::left("combat_log_panel")
        .default_width(320.0)
        .max_width(400.0)
        .min_width(250.0)
        .resizable(true)
        .frame(egui::Frame::side_top_panel(&ctx.style())
            .fill(egui::Color32::from_black_alpha(180))) // Semi-transparent background
        .show(ctx, |ui| {
            ui.heading(
                egui::RichText::new("Combat Log")
                    .size(18.0)
                    .color(egui::Color32::from_rgb(230, 204, 153))
            );
            
            ui.add_space(3.0);
            ui.separator();
            ui.add_space(3.0);
            
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
                            CombatLogEventType::Buff => egui::Color32::from_rgb(180, 220, 255), // Light blue/cyan
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
                                    .size(11.0)
                                    .color(egui::Color32::from_rgb(150, 150, 150))
                            );
                            
                            // Event message in color
                            ui.label(
                                egui::RichText::new(&entry.message)
                                    .size(12.0)
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

/// Spawn visual meshes for newly created spell impact effects.
pub fn spawn_spell_impact_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_effects: Query<(Entity, &SpellImpactEffect), (Added<SpellImpactEffect>, Without<Mesh3d>)>,
) {
    for (effect_entity, effect) in new_effects.iter() {
        // Create a sphere mesh
        let mesh = meshes.add(Sphere::new(effect.initial_scale));
        
        // Purple/shadow color with emissive glow and transparency
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(0.5, 0.2, 0.8, 0.8), // Purple with alpha
            emissive: LinearRgba::rgb(0.8, 0.3, 1.5), // Bright purple/magenta glow
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        
        // Add visual mesh to the effect entity at the target's position
        commands.entity(effect_entity).insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(effect.position + Vec3::new(0.0, 1.0, 0.0)), // Centered at chest height
        ));
    }
}

/// Update spell impact effects: fade and scale them over time.
pub fn update_spell_impact_effects(
    time: Res<Time>,
    mut effects: Query<(&mut SpellImpactEffect, &mut Transform, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dt = time.delta_secs();
    
    for (mut effect, mut transform, material_handle) in effects.iter_mut() {
        effect.lifetime -= dt;
        
        if effect.lifetime <= 0.0 {
            continue; // Will be cleaned up by cleanup system
        }
        
        // Calculate progress (1.0 = just spawned, 0.0 = expired)
        let progress = effect.lifetime / effect.initial_lifetime;
        
        // Scale: expand from initial to final
        let current_scale = effect.initial_scale + (effect.final_scale - effect.initial_scale) * (1.0 - progress);
        transform.scale = Vec3::splat(current_scale);
        
        // Fade out: alpha goes from 1.0 to 0.0
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let alpha = progress * 0.8; // Max alpha 0.8 for translucency
            material.base_color = Color::srgba(0.5, 0.2, 0.8, alpha);
            material.alpha_mode = AlphaMode::Blend;
        }
    }
}

/// Cleanup expired spell impact effects.
pub fn cleanup_expired_spell_impacts(
    mut commands: Commands,
    effects: Query<(Entity, &SpellImpactEffect)>,
) {
    for (entity, effect) in effects.iter() {
        if effect.lifetime <= 0.0 {
            commands.entity(entity).despawn_recursive();
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
    countdown: Res<MatchCountdown>,
    config: Res<match_config::MatchConfig>,
    mut combatants: Query<(Entity, &mut Combatant, &Transform)>,
) {
    // Don't acquire targets until gates open
    if !countdown.gates_opened {
        return;
    }
    // Build list of all alive combatants with their info (excluding stealthed enemies)
    // Also track spawn order for each team to respect kill target priorities
    let mut team1_combatants: Vec<(Entity, Vec3, bool)> = Vec::new();
    let mut team2_combatants: Vec<(Entity, Vec3, bool)> = Vec::new();
    
    for (entity, c, transform) in combatants.iter() {
        if !c.is_alive() {
            continue;
        }
        
        if c.team == 1 {
            team1_combatants.push((entity, transform.translation, c.stealthed));
        } else {
            team2_combatants.push((entity, transform.translation, c.stealthed));
        }
    }

    // For each combatant, ensure they have a valid target
    for (_entity, mut combatant, transform) in combatants.iter_mut() {
        if !combatant.is_alive() {
            combatant.target = None;
            continue;
        }

        // Get enemy team combatants and kill target priority
        let (enemy_combatants, kill_target_index) = if combatant.team == 1 {
            (&team2_combatants, config.team1_kill_target)
        } else {
            (&team1_combatants, config.team2_kill_target)
        };

        // Check if current target is still valid (alive, on enemy team, and not stealthed)
        let target_valid = combatant.target.and_then(|target_entity| {
            enemy_combatants
                .iter()
                .find(|(e, _, _)| *e == target_entity)
                .filter(|(_, _, stealthed)| !stealthed)
        }).is_some();

        // If no valid target, acquire a new one
        if !target_valid {
            // Priority 1: Check if kill target is set and valid
            let kill_target = if let Some(index) = kill_target_index {
                enemy_combatants
                    .get(index)
                    .filter(|(_, _, stealthed)| !stealthed)
                    .map(|(entity, _, _)| *entity)
            } else {
                None
            };
            
            if let Some(priority_target) = kill_target {
                // Use the kill target
                combatant.target = Some(priority_target);
            } else {
                // Priority 2: Fall back to nearest enemy (excluding stealthed)
                let my_pos = transform.translation;
                let nearest_enemy = enemy_combatants
                    .iter()
                    .filter(|(_, _, stealthed)| !stealthed)
                    .min_by(|(_, pos_a, _), (_, pos_b, _)| {
                        let dist_a = my_pos.distance(*pos_a);
                        let dist_b = my_pos.distance(*pos_b);
                        dist_a.partial_cmp(&dist_b).unwrap()
                    });

                combatant.target = nearest_enemy.map(|(entity, _, _)| *entity);
            }
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

/// Find the best direction to move while kiting that maximizes distance from enemy
/// while staying within arena bounds.
/// 
/// Strategy:
/// 1. Try the direct "away from enemy" direction first
/// 2. If that would hit a boundary, test multiple candidate directions
/// 3. Pick the direction that maximizes distance from enemy while staying in bounds
fn find_best_kiting_direction(
    current_pos: Vec3,
    enemy_pos: Vec3,
    move_distance: f32,
) -> Vec3 {
    // Calculate ideal direction (directly away from enemy)
    let ideal_direction = Vec3::new(
        current_pos.x - enemy_pos.x,
        0.0,
        current_pos.z - enemy_pos.z,
    ).normalize_or_zero();
    
    if ideal_direction == Vec3::ZERO {
        return Vec3::ZERO; // Already on top of enemy, can't kite
    }
    
    // Check if ideal direction keeps us in bounds
    let ideal_next_pos = current_pos + ideal_direction * move_distance;
    let ideal_in_bounds = 
        ideal_next_pos.x >= -ARENA_HALF_SIZE && ideal_next_pos.x <= ARENA_HALF_SIZE &&
        ideal_next_pos.z >= -ARENA_HALF_SIZE && ideal_next_pos.z <= ARENA_HALF_SIZE;
    
    if ideal_in_bounds {
        return ideal_direction; // Ideal direction works, use it!
    }
    
    // Ideal direction would hit boundary - find best alternative
    // Test 16 directions around a circle and pick the one that:
    // 1. Stays in bounds
    // 2. Maximizes distance from enemy
    let mut best_direction = Vec3::ZERO;
    let mut best_score = f32::MIN;
    
    for i in 0..16 {
        let angle = (i as f32) * std::f32::consts::TAU / 16.0;
        let candidate_direction = Vec3::new(
            angle.cos(),
            0.0,
            angle.sin(),
        );
        
        // Calculate where we'd end up with this direction
        let candidate_next_pos = current_pos + candidate_direction * move_distance;
        
        // Check if this keeps us in bounds
        let in_bounds = 
            candidate_next_pos.x >= -ARENA_HALF_SIZE && candidate_next_pos.x <= ARENA_HALF_SIZE &&
            candidate_next_pos.z >= -ARENA_HALF_SIZE && candidate_next_pos.z <= ARENA_HALF_SIZE;
        
        if !in_bounds {
            continue; // Skip directions that go out of bounds
        }
        
        // Score this direction based on:
        // 1. Distance from enemy (higher = better)
        // 2. Alignment with ideal direction (bonus for moving away, not sideways)
        let distance_from_enemy = candidate_next_pos.distance(enemy_pos);
        let alignment_with_ideal = candidate_direction.dot(ideal_direction).max(0.0);
        let score = distance_from_enemy * 2.0 + alignment_with_ideal * 5.0;
        
        if score > best_score {
            best_score = score;
            best_direction = candidate_direction;
        }
    }
    
    best_direction
}

pub fn move_to_target(
    countdown: Res<MatchCountdown>,
    time: Res<Time>,
    mut commands: Commands,
    mut combatants: Query<(Entity, &mut Transform, &Combatant, Option<&ActiveAuras>, Option<&CastingState>, Option<&ChargingState>)>,
) {
    // Don't allow movement until gates open
    if !countdown.gates_opened {
        return;
    }
    
    let dt = time.delta_secs();
    
    // Build a snapshot of all combatant positions and team info for lookups
    let positions: std::collections::HashMap<Entity, (Vec3, u8)> = combatants
        .iter()
        .map(|(entity, transform, combatant, _, _, _)| (entity, (transform.translation, combatant.team)))
        .collect();
    
    // Move each combatant towards their target if needed
    for (entity, mut transform, combatant, auras, casting_state, charging_state) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // Cannot move while casting (WoW mechanic)
        if casting_state.is_some() {
            continue;
        }
        
        // Check if rooted or stunned - if so, cannot move
        let is_cc_locked = if let Some(auras) = auras {
            auras.auras.iter().any(|a| matches!(a.effect_type, AuraType::Root | AuraType::Stun))
        } else {
            false
        };
        
        if is_cc_locked {
            continue;
        }
        
        let my_pos = transform.translation;
        
        // CHARGING BEHAVIOR: If charging, move at high speed toward target ignoring slows
        if let Some(charge_state) = charging_state {
            let Some(&(target_pos, _)) = positions.get(&charge_state.target) else {
                // Target doesn't exist, cancel charge
                commands.entity(entity).remove::<ChargingState>();
                continue;
            };
            
            let distance = my_pos.distance(target_pos);
            
            // If we've reached melee range, end the charge
            if distance <= MELEE_RANGE {
                commands.entity(entity).remove::<ChargingState>();
                
                info!(
                    "Team {} {} completes charge!",
                    combatant.team,
                    combatant.class.name()
                );
                
                continue; // Will use normal movement/combat next frame
            }
            
            // Calculate direction to target
            let direction = Vec3::new(
                target_pos.x - my_pos.x,
                0.0,
                target_pos.z - my_pos.z,
            ).normalize_or_zero();
            
            if direction != Vec3::ZERO {
                // Charge speed: 4x normal movement speed, ignores slows
                const CHARGE_SPEED_MULTIPLIER: f32 = 4.0;
                let charge_speed = combatant.base_movement_speed * CHARGE_SPEED_MULTIPLIER;
                let move_distance = charge_speed * dt;
                
                // Move towards target
                transform.translation += direction * move_distance;
                
                // Clamp position to arena bounds
                transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                
                // Rotate to face target
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }
            
            continue; // Skip normal movement logic while charging
        }
        
        // KITING BEHAVIOR: If kiting_timer > 0, move away from nearest enemy
        // Uses intelligent pathfinding that considers arena boundaries
        if combatant.kiting_timer > 0.0 {
            // Find nearest enemy
            let mut nearest_enemy_pos: Option<Vec3> = None;
            let mut nearest_distance = f32::MAX;
            
            for (other_entity, &(other_pos, other_team)) in positions.iter() {
                if *other_entity != entity && other_team != combatant.team {
                    let distance = my_pos.distance(other_pos);
                    if distance < nearest_distance {
                        nearest_distance = distance;
                        nearest_enemy_pos = Some(other_pos);
                    }
                }
            }
            
            // Intelligent kiting: maximize distance from nearest enemy
            if let Some(enemy_pos) = nearest_enemy_pos {
                // Calculate effective movement speed (base * aura modifiers)
                let mut movement_speed = combatant.base_movement_speed;
                if let Some(auras) = auras {
                    for aura in &auras.auras {
                        if aura.effect_type == AuraType::MovementSpeedSlow {
                            movement_speed *= aura.magnitude;
                        }
                    }
                }
                
                let move_distance = movement_speed * dt;
                
                // Find the best direction to move that maximizes distance from enemy
                // while staying within arena bounds
                let best_direction = find_best_kiting_direction(
                    my_pos,
                    enemy_pos,
                    move_distance,
                );
                
                if best_direction != Vec3::ZERO {
                    // Move in the best direction
                    transform.translation += best_direction * move_distance;
                    
                    // Ensure we stay in bounds (in case of floating point errors)
                    transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                    transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                    
                    // Rotate to face direction of travel
                    let target_rotation = Quat::from_rotation_y(best_direction.x.atan2(best_direction.z));
                    transform.rotation = target_rotation;
                }
            }
            
            continue; // Skip normal movement logic
        }
        
        // NORMAL MOVEMENT: Get target position
        let Some(target_entity) = combatant.target else {
            // No target available (likely facing all-stealth team)
            // Move to defensive position in center of arena to anticipate stealth openers
            let defensive_pos = Vec3::ZERO; // Center of arena
            let distance_to_defensive = my_pos.distance(defensive_pos);
            
            // Only move if we're far from the defensive position (> 5 units)
            if distance_to_defensive > 5.0 {
                let direction = Vec3::new(
                    defensive_pos.x - my_pos.x,
                    0.0,
                    defensive_pos.z - my_pos.z,
                ).normalize_or_zero();
                
                if direction != Vec3::ZERO {
                    // Calculate effective movement speed
                    let mut movement_speed = combatant.base_movement_speed;
                    if let Some(auras) = auras {
                        for aura in &auras.auras {
                            if aura.effect_type == AuraType::MovementSpeedSlow {
                                movement_speed *= aura.magnitude;
                            }
                        }
                    }
                    
                    // Move towards defensive position
                    let move_distance = movement_speed * dt;
                    transform.translation += direction * move_distance;
                    
                    // Clamp position to arena bounds
                    transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                    transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                    
                    // Rotate to face center
                    let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                    transform.rotation = target_rotation;
                }
            }
            
            continue;
        };
        
        let Some(&(target_pos, _)) = positions.get(&target_entity) else {
            continue;
        };
        
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
                
                // Clamp position to arena bounds
                transform.translation.x = transform.translation.x.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                transform.translation.z = transform.translation.z.clamp(-ARENA_HALF_SIZE, ARENA_HALF_SIZE);
                
                // Rotate to face target
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }
        }
    }
}

/// Update visual appearance of stealthed combatants.
/// 
/// Makes stealthed Rogues semi-transparent (40% alpha) with a darker tint
/// to clearly indicate their stealth status. When they break stealth (e.g., by using Ambush),
/// they return to full opacity and original color.
pub fn update_stealth_visuals(
    combatants: Query<(&Combatant, &MeshMaterial3d<StandardMaterial>), Changed<Combatant>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (combatant, material_handle) in combatants.iter() {
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let current_color = material.base_color.to_srgba();
            let current_alpha = current_color.alpha;
            
            if combatant.stealthed {
                // Only apply stealth effect if not already stealthed (alpha is 1.0)
                if current_alpha >= 0.9 {
                    // Semi-transparent with darker tint for stealth
                    let color = Color::srgba(
                        current_color.red * 0.6,
                        current_color.green * 0.6,
                        current_color.blue * 0.6,
                        0.4, // 40% opacity
                    );
                    material.base_color = color;
                }
            } else {
                // Only restore if currently stealthed (alpha is low)
                if current_alpha < 0.9 {
                    // Restore original color by reversing the darkening (divide by 0.6)
                    let color = Color::srgba(
                        (current_color.red / 0.6).min(1.0),
                        (current_color.green / 0.6).min(1.0),
                        (current_color.blue / 0.6).min(1.0),
                        1.0, // Full opacity
                    );
                    material.base_color = color;
                }
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
    countdown: Res<MatchCountdown>,
    time: Res<Time>,
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut combatants: Query<(Entity, &Transform, &mut Combatant, Option<&CastingState>, Option<&ActiveAuras>)>,
) {
    let dt = time.delta_secs();
    
    // Update match time in combat log (countdown doesn't count against match time)
    if countdown.gates_opened {
        combat_log.match_time += dt;
    }
    
    // Don't allow auto-attacks until gates open
    if !countdown.gates_opened {
        return;
    }
    
    // Build a snapshot of positions for range checks
    let positions: std::collections::HashMap<Entity, Vec3> = combatants
        .iter()
        .map(|(entity, transform, _, _, _)| (entity, transform.translation))
        .collect();
    
    // Build a snapshot of combatant info for logging
    let combatant_info: std::collections::HashMap<Entity, (u8, match_config::CharacterClass)> = combatants
        .iter()
        .map(|(entity, _, combatant, _, _)| (entity, (combatant.team, combatant.class)))
        .collect();
    
    // Collect attacks that will happen this frame (attacker, target, damage)
    let mut attacks = Vec::new();
    
    // Track damage per target for batching floating combat text
    let mut damage_per_target: std::collections::HashMap<Entity, f32> = std::collections::HashMap::new();
    // Track damage per target for aura breaking
    let mut damage_per_aura_break: std::collections::HashMap<Entity, f32> = std::collections::HashMap::new();
    
    for (attacker_entity, transform, mut combatant, casting_state, auras) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // WoW Mechanic: Cannot auto-attack while stunned
        let is_stunned = if let Some(auras) = auras {
            auras.auras.iter().any(|a| a.effect_type == AuraType::Stun)
        } else {
            false
        };
        if is_stunned {
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
        if let Ok((_, _, mut target, _, _)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                let actual_damage = damage.min(target.current_health);
                target.current_health = (target.current_health - damage).max(0.0);
                target.damage_taken += actual_damage;
                
                // Warriors generate Rage from taking damage
                if target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15; // Gain 15% of damage taken as Rage
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }
                
                // Track damage for aura breaking
                *damage_per_aura_break.entry(target_entity).or_insert(0.0) += actual_damage;
                
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
        if let Ok((_, _, mut attacker, _, _)) = combatants.get_mut(attacker_entity) {
            attacker.damage_dealt += damage;
        }
    }
    
    // Track damage for aura breaking
    for (target_entity, total_damage) in damage_per_aura_break {
        commands.entity(target_entity).insert(DamageTakenThisFrame {
            amount: total_damage,
        });
    }
}

/// Resource regeneration system: Regenerate mana for all combatants.
/// 
/// Each combatant with mana regeneration gains mana per second up to their max.
/// Also ticks down ability cooldowns over time.
pub fn regenerate_resources(
    time: Res<Time>,
    mut combatants: Query<&mut Combatant>,
) {
    let dt = time.delta_secs();
    
    for mut combatant in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // Regenerate mana/resources
        if combatant.mana_regen > 0.0 {
            combatant.current_mana = (combatant.current_mana + combatant.mana_regen * dt).min(combatant.max_mana);
        }
        
        // Tick down ability cooldowns
        let abilities_on_cooldown: Vec<AbilityType> = combatant.ability_cooldowns.keys().copied().collect();
        for ability in abilities_on_cooldown {
            if let Some(cooldown) = combatant.ability_cooldowns.get_mut(&ability) {
                *cooldown -= dt;
                if *cooldown <= 0.0 {
                    combatant.ability_cooldowns.remove(&ability);
                }
            }
        }
        
        // Tick down global cooldown
        if combatant.global_cooldown > 0.0 {
            combatant.global_cooldown -= dt;
            if combatant.global_cooldown < 0.0 {
                combatant.global_cooldown = 0.0;
            }
        }
        
        // Tick down kiting timer
        if combatant.kiting_timer > 0.0 {
            combatant.kiting_timer -= dt;
            if combatant.kiting_timer < 0.0 {
                combatant.kiting_timer = 0.0;
            }
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
    mut combatants: Query<(Entity, &mut Combatant, &Transform, Option<&ActiveAuras>), Without<CastingState>>,
) {
    // Build position and info maps from all combatants
    let positions: std::collections::HashMap<Entity, Vec3> = combatants
        .iter()
        .map(|(entity, _, transform, _)| (entity, transform.translation))
        .collect();
    
    let combatant_info: std::collections::HashMap<Entity, (u8, match_config::CharacterClass, f32, f32)> = combatants
        .iter()
        .map(|(entity, combatant, _, _)| {
            (entity, (combatant.team, combatant.class, combatant.current_health, combatant.max_health))
        })
        .collect();
    
    // Map of entities to their active auras (for checking buffs/debuffs)
    let active_auras_map: std::collections::HashMap<Entity, Vec<Aura>> = combatants
        .iter()
        .filter_map(|(entity, _, _, auras_opt)| {
            auras_opt.map(|auras| (entity, auras.auras.clone()))
        })
        .collect();
    
    // Queue for Ambush attacks (attacker, target, damage, team, class)
    // Queue for instant ability attacks (Ambush, Sinister Strike)
    // Format: (attacker_entity, target_entity, damage, attacker_team, attacker_class, ability_type)
    let mut instant_attacks: Vec<(Entity, Entity, f32, u8, match_config::CharacterClass, AbilityType)> = Vec::new();
    
    // Queue for Frost Nova damage (caster, target, damage, caster_team, caster_class, target_pos)
    let mut frost_nova_damage: Vec<(Entity, Entity, f32, u8, match_config::CharacterClass, Vec3)> = Vec::new();
    
    for (entity, mut combatant, transform, auras) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // WoW Mechanic: Cannot use abilities while stunned
        let is_stunned = if let Some(auras) = auras {
            auras.auras.iter().any(|a| a.effect_type == AuraType::Stun)
        } else {
            false
        };
        if is_stunned {
            continue;
        }
        
        let my_pos = transform.translation;
        
        // Mages cast spells on enemies
        if combatant.class == match_config::CharacterClass::Mage {
            // Check if global cooldown is active
            if combatant.global_cooldown > 0.0 {
                continue; // Can't use abilities during GCD
            }
            
            // First priority: Use Frost Nova if enemies are in melee range (defensive ability)
            let frost_nova = AbilityType::FrostNova;
            let nova_def = frost_nova.definition();
            let nova_on_cooldown = combatant.ability_cooldowns.contains_key(&frost_nova);
            
            if !nova_on_cooldown && combatant.current_mana >= nova_def.mana_cost {
                // Check if any enemies are within Frost Nova range (melee range for threat detection)
                let enemies_in_melee_range = positions.iter().any(|(enemy_entity, &enemy_pos)| {
                    if let Some(&(enemy_team, _, _, _)) = combatant_info.get(enemy_entity) {
                        if enemy_team != combatant.team {
                            let distance = my_pos.distance(enemy_pos);
                            return distance <= MELEE_RANGE;
                        }
                    }
                    false
                });
                
                if enemies_in_melee_range {
                    // Consume mana
                    combatant.current_mana -= nova_def.mana_cost;
                    
                    // Put ability on cooldown
                    combatant.ability_cooldowns.insert(frost_nova, nova_def.cooldown);
                    
                    // Trigger global cooldown (1.5s standard WoW GCD)
                    combatant.global_cooldown = 1.5;
                    
                    // Collect enemies in range for damage and root
                    let mut frost_nova_targets: Vec<(Entity, Vec3, u8, match_config::CharacterClass)> = Vec::new();
                    for (enemy_entity, &enemy_pos) in positions.iter() {
                        if let Some(&(enemy_team, enemy_class, _, _)) = combatant_info.get(enemy_entity) {
                            if enemy_team != combatant.team {
                                let distance = my_pos.distance(enemy_pos);
                                if distance <= nova_def.range {
                                    frost_nova_targets.push((*enemy_entity, enemy_pos, enemy_team, enemy_class));
                                }
                            }
                        }
                    }
                    
                    // Queue damage and apply root to all targets
                    for (target_entity, target_pos, target_team, target_class) in &frost_nova_targets {
                        // Calculate damage (with stat scaling)
                        let damage = combatant.calculate_ability_damage(&nova_def);
                        
                        // Queue damage for later application
                        frost_nova_damage.push((entity, *target_entity, damage, combatant.team, combatant.class, *target_pos));
                        
                        // Apply aura (spawn separate AuraPending entity)
                        if let Some((aura_type, duration, magnitude, break_threshold)) = nova_def.applies_aura {
                            commands.spawn(AuraPending {
                                target: *target_entity,
                                aura: Aura {
                                    effect_type: aura_type,
                                    duration,
                                    magnitude,
                                    break_on_damage_threshold: break_threshold,
                                    accumulated_damage: 0.0,
                                    tick_interval: 0.0,
                                    time_until_next_tick: 0.0,
                                    caster: Some(entity),
                                },
                            });
                        }
                    }
                    
                    // Set kiting timer - mage should move away from enemies for the root duration
                    combatant.kiting_timer = nova_def.applies_aura.unwrap().1; // Root duration (6.0s)
                    
                    info!(
                        "Team {} {} casts Frost Nova! (AOE root) - {} enemies affected",
                        combatant.team,
                        combatant.class.name(),
                        frost_nova_targets.len()
                    );
                    
                    continue; // Don't cast Frostbolt this frame
                }
            }
            
            // Second priority: Cast Frostbolt on target
            // While kiting, only cast if we're at a safe distance (beyond melee range + buffer)
            let Some(target_entity) = combatant.target else {
                continue;
            };
            
            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };
            
            let distance_to_target = my_pos.distance(target_pos);
            
            // While kiting, only cast if we're at a safe distance
            // Safe distance = beyond melee range + buffer (8 units gives good tactical spacing)
            const SAFE_KITING_DISTANCE: f32 = 8.0;
            if combatant.kiting_timer > 0.0 && distance_to_target < SAFE_KITING_DISTANCE {
                continue; // Too close while kiting, focus on movement
            }
            
            // Check if global cooldown is active
            if combatant.global_cooldown > 0.0 {
                continue; // Can't start casting during GCD
            }
            
            // Try to cast Frostbolt
            let ability = AbilityType::Frostbolt;
            let def = ability.definition();
            
            // Check if spell school is locked out
            let is_locked_out = if let Some(auras) = auras {
                auras.auras.iter().any(|aura| {
                    if aura.effect_type == AuraType::SpellSchoolLockout {
                        // Convert magnitude back to spell school
                        let locked_school = match aura.magnitude as u8 {
                            0 => SpellSchool::Physical,
                            1 => SpellSchool::Frost,
                            2 => SpellSchool::Holy,
                            3 => SpellSchool::Shadow,
                            _ => SpellSchool::None,
                        };
                        locked_school == def.spell_school
                    } else {
                        false
                    }
                })
            } else {
                false
            };
            
            if is_locked_out {
                continue; // Can't cast - spell school is locked
            }
            
            if ability.can_cast(&combatant, target_pos, my_pos) {
                let def = ability.definition();
                
                // Trigger global cooldown (1.5s standard WoW GCD)
                // GCD starts when cast BEGINS, not when it completes
                combatant.global_cooldown = 1.5;
                
                // Start casting
                commands.entity(entity).insert(CastingState {
                    ability,
                    time_remaining: def.cast_time,
                    target: Some(target_entity),
                    interrupted: false,
                    interrupted_display_time: 0.0,
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
            // Check if global cooldown is active (check once for all abilities)
            if combatant.global_cooldown > 0.0 {
                continue; // Can't cast during GCD
            }
            
            // Priority 0: Cast Power Word: Fortitude on allies who don't have it
            // (Pre-combat buffing phase)
            let mut unbuffed_ally: Option<(Entity, Vec3)> = None;
            
            for (ally_entity, &(ally_team, _ally_class, ally_hp, _ally_max_hp)) in combatant_info.iter() {
                // Must be same team and alive
                if ally_team != combatant.team || ally_hp <= 0.0 {
                    continue;
                }
                
                // Check if ally already has MaxHealthIncrease buff
                let has_fortitude = if let Some(auras) = active_auras_map.get(ally_entity) {
                    auras.iter().any(|a| a.effect_type == AuraType::MaxHealthIncrease)
                } else {
                    false
                };
                
                if has_fortitude {
                    continue; // Already buffed
                }
                
                // Get position
                let Some(&ally_pos) = positions.get(ally_entity) else {
                    continue;
                };
                
                // Found an unbuffed ally
                unbuffed_ally = Some((*ally_entity, ally_pos));
                break; // Buff one ally at a time
            }
            
            // Cast Fortitude on unbuffed ally
            if let Some((buff_target, target_pos)) = unbuffed_ally {
                let ability = AbilityType::PowerWordFortitude;
                if ability.can_cast(&combatant, target_pos, my_pos) {
                    let def = ability.definition();
                    
                    // Consume mana
                    combatant.current_mana -= def.mana_cost;
                    
                    // Trigger global cooldown
                    combatant.global_cooldown = 1.5;
                    
                    // Apply the buff aura immediately (instant cast)
                    if let Some((aura_type, duration, magnitude, break_threshold)) = def.applies_aura {
                        commands.spawn(AuraPending {
                            target: buff_target,
                            aura: Aura {
                                effect_type: aura_type,
                                duration,
                                magnitude,
                                break_on_damage_threshold: break_threshold,
                                accumulated_damage: 0.0,
                                tick_interval: 0.0,
                                time_until_next_tick: 0.0,
                                caster: Some(entity),
                            },
                        });
                    }
                    
                    info!(
                        "Team {} {} casts Power Word: Fortitude on ally",
                        combatant.team,
                        combatant.class.name()
                    );
                    
                    continue; // Done this frame
                }
            }
            
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
            
            // Priority 1: Cast heal on lowest HP ally if found
            if let Some((heal_target, _, target_pos)) = lowest_hp_ally {
                let ability = AbilityType::FlashHeal;
                if ability.can_cast(&combatant, target_pos, my_pos) {
                    let def = ability.definition();
                    
                    // Trigger global cooldown (1.5s standard WoW GCD)
                    // GCD starts when cast BEGINS, not when it completes
                    combatant.global_cooldown = 1.5;
                    
                    // Start casting
                    commands.entity(entity).insert(CastingState {
                        ability,
                        time_remaining: def.cast_time,
                        target: Some(heal_target),
                        interrupted: false,
                        interrupted_display_time: 0.0,
                    });
                    
                    info!(
                        "Team {} {} starts casting {} on ally",
                        combatant.team,
                        combatant.class.name(),
                        def.name
                    );
                    
                    continue; // Done this frame
                }
            }
            
            // Priority 2: Cast Mind Blast on enemy if no healing needed
            let Some(target_entity) = combatant.target else {
                continue;
            };
            
            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };
            
            // Check if Mind Blast is off cooldown
            let ability = AbilityType::MindBlast;
            let on_cooldown = combatant.ability_cooldowns.contains_key(&ability);
            
            if !on_cooldown && ability.can_cast(&combatant, target_pos, my_pos) {
                let def = ability.definition();
                
                // Put on cooldown
                combatant.ability_cooldowns.insert(ability, def.cooldown);
                
                // Trigger global cooldown (1.5s standard WoW GCD)
                // GCD starts when cast BEGINS, not when it completes
                combatant.global_cooldown = 1.5;
                
                // Start casting
                commands.entity(entity).insert(CastingState {
                    ability,
                    time_remaining: def.cast_time,
                    target: Some(target_entity),
                    interrupted: false,
                    interrupted_display_time: 0.0,
                });
                
                info!(
                    "Team {} {} starts casting {} on enemy",
                    combatant.team,
                    combatant.class.name(),
                    def.name
                );
            }
        }
        
        // Warriors use Charge (gap closer) and Heroic Strike (damage)
        if combatant.class == match_config::CharacterClass::Warrior {
            // Check if we have an enemy target
            let Some(target_entity) = combatant.target else {
                continue;
            };
            
            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };
            
            let distance_to_target = my_pos.distance(target_pos);
            
            // NOTE: Interrupt checking (Pummel) is now handled in the dedicated check_interrupts system
            // which runs after apply_deferred so it can see CastingState components from this frame
            
            // Check if global cooldown is active for other abilities
            if combatant.global_cooldown > 0.0 {
                continue; // Can't use other abilities during GCD
            }
            
            // Priority 1: Use Charge to close distance if target is at medium range
            // Charge requirements:
            // - Minimum 8 units (can't waste at melee range)
            // - Maximum 25 units (ability range)
            // - Not rooted (can't charge while rooted)
            // - Off cooldown
            const CHARGE_MIN_RANGE: f32 = 8.0;
            let charge = AbilityType::Charge;
            let charge_def = charge.definition();
            let charge_on_cooldown = combatant.ability_cooldowns.contains_key(&charge);
            
            // Check if rooted
            let is_rooted = if let Some(auras) = auras {
                auras.auras.iter().any(|aura| matches!(aura.effect_type, AuraType::Root))
            } else {
                false
            };
            
            if !charge_on_cooldown 
                && !is_rooted
                && distance_to_target >= CHARGE_MIN_RANGE 
                && distance_to_target <= charge_def.range {
                
                // Use Charge!
                combatant.ability_cooldowns.insert(charge, charge_def.cooldown);
                combatant.global_cooldown = 1.5;
                
                // Add ChargingState component to enable high-speed movement
                commands.entity(entity).insert(ChargingState {
                    target: target_entity,
                });
                
                info!(
                    "Team {} {} uses {} on enemy (distance: {:.1} units)",
                    combatant.team,
                    combatant.class.name(),
                    charge_def.name,
                    distance_to_target
                );
                
                continue; // Done this frame
            }
            
            // Priority 2: Apply Rend if target doesn't have it
            let target_has_rend = if let Some(auras) = active_auras_map.get(&target_entity) {
                auras.iter().any(|a| a.effect_type == AuraType::DamageOverTime)
            } else {
                false
            };
            
            if !target_has_rend {
                let rend = AbilityType::Rend;
                let rend_def = rend.definition();
                let can_cast_rend = rend.can_cast(&combatant, target_pos, my_pos);
                
                if can_cast_rend {
                    // Consume rage
                    combatant.current_mana -= rend_def.mana_cost;
                    
                    // Trigger global cooldown
                    combatant.global_cooldown = 1.5;
                    
                    // Apply the DoT aura
                    if let Some((aura_type, duration, magnitude, break_threshold)) = rend_def.applies_aura {
                        commands.spawn(AuraPending {
                            target: target_entity,
                            aura: Aura {
                                effect_type: aura_type,
                                duration,
                                magnitude,
                                break_on_damage_threshold: break_threshold,
                                accumulated_damage: 0.0,
                                tick_interval: 3.0, // Tick every 3 seconds
                                time_until_next_tick: 3.0, // First tick after 3 seconds
                                caster: Some(entity),
                            },
                        });
                    }
                    
                    // Log Rend application to combat log
                    combat_log.log(
                        CombatLogEventType::Buff,
                        format!(
                            "Team {} {} applies Rend to enemy (8 damage per 3s for 15s)",
                            combatant.team,
                            combatant.class.name()
                        )
                    );
                    
                    info!(
                        "Team {} {} applies Rend to enemy (8 damage per 3s for 15s)",
                        combatant.team,
                        combatant.class.name()
                    );
                    
                    continue; // Done this frame
                }
            }
            
            // Priority 3: Use Heroic Strike if target is in melee range
            // Only use Heroic Strike if we have excess rage (save rage for Rend/Pummel)
            // Don't queue another Heroic Strike if one is already pending
            if combatant.next_attack_bonus_damage > 0.0 {
                continue;
            }
            
            // Try to use Heroic Strike if we have enough rage and target is in melee range
            let ability = AbilityType::HeroicStrike;
            let def = ability.definition();
            
            // Only use if we have enough rage for both Heroic Strike AND Rend+Pummel reserve
            // Reserve: 10 (Rend) + 10 (Pummel) = 20 rage minimum
            const RAGE_RESERVE: f32 = 20.0;
            let can_afford_heroic_strike = combatant.current_mana >= (def.mana_cost + RAGE_RESERVE);
            
            if can_afford_heroic_strike && ability.can_cast(&combatant, target_pos, my_pos) {
                // Since it's instant, apply the effect immediately
                // Consume rage
                combatant.current_mana -= def.mana_cost;
                
                // Set bonus damage for next auto-attack (50% of base attack damage)
                let bonus_damage = combatant.attack_damage * 0.5;
                combatant.next_attack_bonus_damage = bonus_damage;
                
                // Trigger global cooldown (1.5s standard WoW GCD)
                combatant.global_cooldown = 1.5;
                
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
                
                // Calculate damage (with stat scaling)
                let damage = combatant.calculate_ability_damage(&def);
                
                // Queue the Ambush attack to be applied after the loop
                instant_attacks.push((entity, target_entity, damage, combatant.team, combatant.class, ability));
                
                // Trigger global cooldown (1.5s standard WoW GCD)
                combatant.global_cooldown = 1.5;
                
                info!(
                    "Team {} {} uses {} from stealth!",
                    combatant.team,
                    combatant.class.name(),
                    def.name
                );
            }
        }
        
        // Rogues use Kick, Kidney Shot and Sinister Strike when out of stealth
        if combatant.class == match_config::CharacterClass::Rogue && !combatant.stealthed {
            // Check if we have an enemy target
            let Some(target_entity) = combatant.target else {
                continue;
            };
            
            let Some(&target_pos) = positions.get(&target_entity) else {
                continue;
            };
            
            // NOTE: Interrupt checking (Kick) is now handled in the dedicated check_interrupts system
            // which runs after apply_deferred so it can see CastingState components from this frame
            
            // Check if global cooldown is active for other abilities
            if combatant.global_cooldown > 0.0 {
                continue; // Can't use other abilities during GCD
            }
            
            // Priority 1: Use Kidney Shot (stun) if available
            let kidney_shot = AbilityType::KidneyShot;
            let ks_on_cooldown = combatant.ability_cooldowns.contains_key(&kidney_shot);
            
            if !ks_on_cooldown && kidney_shot.can_cast(&combatant, target_pos, my_pos) {
                let def = kidney_shot.definition();
                
                // Consume energy
                combatant.current_mana -= def.mana_cost;
                
                // Put on cooldown
                combatant.ability_cooldowns.insert(kidney_shot, def.cooldown);
                
                // Trigger global cooldown
                combatant.global_cooldown = 1.5;
                
                // Spawn pending aura (stun effect)
                if let Some((aura_type, duration, magnitude, break_threshold)) = def.applies_aura {
                    commands.spawn(AuraPending {
                        target: target_entity,
                        aura: Aura {
                            effect_type: aura_type,
                            duration,
                            magnitude,
                            break_on_damage_threshold: break_threshold,
                            accumulated_damage: 0.0,
                            tick_interval: 0.0,
                            time_until_next_tick: 0.0,
                            caster: Some(entity),
                        },
                    });
                }
                
                info!(
                    "Team {} {} uses {} on enemy!",
                    combatant.team,
                    combatant.class.name(),
                    def.name
                );
                
                // Log to combat log
                let message = format!(
                    "Team {} {} uses {} on Team {} {}",
                    combatant.team,
                    combatant.class.name(),
                    def.name,
                    combatant_info.get(&target_entity).map(|(t, _, _, _)| *t).unwrap_or(0),
                    combatant_info.get(&target_entity).map(|(_, c, _, _)| c.name()).unwrap_or("Unknown")
                );
                combat_log.log(CombatLogEventType::CrowdControl, message);
                
                continue; // Done this frame
            }
            
            // Priority 2: Use Sinister Strike if we have enough energy and target is in melee range
            let ability = AbilityType::SinisterStrike;
            if ability.can_cast(&combatant, target_pos, my_pos) {
                let def = ability.definition();
                
                // Consume energy
                combatant.current_mana -= def.mana_cost;
                
                // Calculate damage (with stat scaling)
                let damage = combatant.calculate_ability_damage(&def);
                
                // Queue the Sinister Strike attack to be applied after the loop
                instant_attacks.push((entity, target_entity, damage, combatant.team, combatant.class, ability));
                
                // Trigger global cooldown (1.5s standard WoW GCD)
                combatant.global_cooldown = 1.5;
                
                info!(
                    "Team {} {} uses {}!",
                    combatant.team,
                    combatant.class.name(),
                    def.name
                );
            }
        }
    }
    
    // Process queued instant attacks (Ambush, Sinister Strike)
    for (attacker_entity, target_entity, damage, attacker_team, attacker_class, ability) in instant_attacks {
        let ability_name = ability.definition().name;
        let mut actual_damage = 0.0;
        let mut target_team = 0;
        let mut target_class = match_config::CharacterClass::Warrior; // Default, will be overwritten
        
        if let Ok((_, mut target, target_transform, _)) = combatants.get_mut(target_entity) {
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
                
                // Track damage for aura breaking
                commands.entity(target_entity).insert(DamageTakenThisFrame {
                    amount: actual_damage,
                });
                
                info!(
                    "Team {} {}'s {} hits Team {} {} for {:.0} damage!",
                    attacker_team,
                    attacker_class.name(),
                    ability_name,
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
                
                // Log the instant attack with position data
                let message = format!(
                    "Team {} {}'s {} hits Team {} {} for {:.0} damage",
                    attacker_team,
                    attacker_class.name(),
                    ability_name,
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
        if let Ok((_, mut attacker, _, _)) = combatants.get_mut(attacker_entity) {
            attacker.damage_dealt += actual_damage;
        }
    }
    
    // Process queued Frost Nova damage
    for (caster_entity, target_entity, damage, caster_team, caster_class, target_pos) in frost_nova_damage {
        let mut actual_damage = 0.0;
        let mut target_team = 0;
        let mut target_class = match_config::CharacterClass::Warrior;
        
        if let Ok((_, mut target, target_transform, _)) = combatants.get_mut(target_entity) {
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
                
                // Track damage for aura breaking
                commands.entity(target_entity).insert(DamageTakenThisFrame {
                    amount: actual_damage,
                });
                
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
                
                // Log the Frost Nova damage with position data
                let message = format!(
                    "Team {} {}'s Frost Nova hits Team {} {} for {:.0} damage",
                    caster_team,
                    caster_class.name(),
                    target_team,
                    target_class.name(),
                    actual_damage
                );
                
                if let Some(&caster_pos) = positions.get(&caster_entity) {
                    let distance = caster_pos.distance(target_pos);
                    combat_log.log_with_position(
                        CombatLogEventType::Damage,
                        message,
                        PositionData {
                            entities: vec![
                                format!("Team {} {} (caster)", caster_team, caster_class.name()),
                                format!("Team {} {} (target)", target_team, target_class.name()),
                            ],
                            positions: vec![
                                (caster_pos.x, caster_pos.y, caster_pos.z),
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
        
        // Update caster's damage dealt
        if let Ok((_, mut caster, _, _)) = combatants.get_mut(caster_entity) {
            caster.damage_dealt += actual_damage;
        }
    }
}

/// Casting system: Process active casts, complete them when time is up.
/// Check if any combatants should interrupt their targets
/// This runs AFTER apply_deferred so it can see CastingState components added this frame
pub fn check_interrupts(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut combatants: Query<(Entity, &mut Combatant, &Transform), Without<CastingState>>,
    casting_targets: Query<&CastingState>,
    positions: Query<&Transform>,
) {
    for (entity, mut combatant, transform) in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        // Only Warriors and Rogues have interrupts
        if combatant.class != match_config::CharacterClass::Warrior 
            && combatant.class != match_config::CharacterClass::Rogue {
            continue;
        }
        
        let Some(target_entity) = combatant.target else {
            continue;
        };
        
        let Ok(target_transform) = positions.get(target_entity) else {
            continue;
        };
        
        let my_pos = transform.translation;
        let target_pos = target_transform.translation;
        let distance = my_pos.distance(target_pos);
        
        // Check if target is casting
        let Ok(cast_state) = casting_targets.get(target_entity) else {
            continue; // Target not casting
        };
        
        if cast_state.interrupted {
            continue; // Already interrupted
        }
        
        // Determine which interrupt ability to use based on class
        let interrupt_ability = match combatant.class {
            match_config::CharacterClass::Warrior => AbilityType::Pummel,
            match_config::CharacterClass::Rogue => AbilityType::Kick,
            _ => continue,
        };
        
        let ability_def = interrupt_ability.definition();
        
        // Check if interrupt is on cooldown
        if combatant.ability_cooldowns.contains_key(&interrupt_ability) {
            continue;
        }
        
        // Check if we can cast the interrupt (range, resources, etc.)
        if !interrupt_ability.can_cast(&combatant, target_pos, my_pos) {
            continue;
        }
        
        // Use the interrupt!
        info!(
            "[INTERRUPT] Team {} {} uses {} to interrupt {}'s cast (distance: {:.1}, time_remaining: {:.2}s)",
            combatant.team,
            combatant.class.name(),
            ability_def.name,
            cast_state.ability.definition().name,
            distance,
            cast_state.time_remaining
        );
        
        // Consume resources
        combatant.current_mana -= ability_def.mana_cost;
        
        // Put on cooldown
        combatant.ability_cooldowns.insert(interrupt_ability, ability_def.cooldown);
        
        // Interrupts do NOT trigger GCD in WoW!
        
        // Queue the interrupt for processing
        commands.spawn(InterruptPending {
            caster: entity,
            target: target_entity,
            ability: interrupt_ability,
            lockout_duration: ability_def.lockout_duration,
        });
        
        // Log to combat log
        combat_log.log(
            CombatLogEventType::AbilityUsed,
            format!(
                "Team {} {} uses {} to interrupt enemy cast",
                combatant.team,
                combatant.class.name(),
                ability_def.name
            )
        );
    }
}

/// 
/// Reduces cast timers each frame. When a cast completes:
/// Process interrupt attempts: interrupt target's cast and apply spell school lockout.
pub fn process_interrupts(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    interrupts: Query<(Entity, &InterruptPending)>,
    mut targets: Query<(&mut CastingState, &Combatant)>,
    combatants: Query<&Combatant>,
) {
    for (interrupt_entity, interrupt) in interrupts.iter() {
        // Check if target is still casting
        if let Ok((mut cast_state, target_combatant)) = targets.get_mut(interrupt.target) {
            // Don't interrupt if already interrupted
            if cast_state.interrupted {
                commands.entity(interrupt_entity).despawn();
                continue;
            }
            
            // Get the spell school of the interrupted spell
            let interrupted_ability_def = cast_state.ability.definition();
            let interrupted_school = interrupted_ability_def.spell_school;
            let interrupted_spell_name = interrupted_ability_def.name;
            
            // Mark cast as interrupted
            cast_state.interrupted = true;
            cast_state.interrupted_display_time = 0.5; // Show "INTERRUPTED" for 0.5 seconds
            
            // Get caster info for logging
            let caster_info = if let Ok(caster) = combatants.get(interrupt.caster) {
                (caster.team, caster.class)
            } else {
                (0, match_config::CharacterClass::Warrior) // Fallback
            };
            
            // Apply spell school lockout aura
            // Store the locked school as the magnitude (cast to f32)
            let locked_school_value = match interrupted_school {
                SpellSchool::Physical => 0.0,
                SpellSchool::Frost => 1.0,
                SpellSchool::Holy => 2.0,
                SpellSchool::Shadow => 3.0,
                SpellSchool::None => 4.0,
            };
            
            commands.spawn(AuraPending {
                target: interrupt.target,
                aura: Aura {
                    effect_type: AuraType::SpellSchoolLockout,
                    duration: interrupt.lockout_duration,
                    magnitude: locked_school_value,
                    break_on_damage_threshold: 0.0,
                    accumulated_damage: 0.0,
                    tick_interval: 0.0,
                    time_until_next_tick: 0.0,
                    caster: Some(interrupt.caster),
                },
            });
            
            // Log the interrupt
            let school_name = match interrupted_school {
                SpellSchool::Physical => "Physical",
                SpellSchool::Frost => "Frost",
                SpellSchool::Holy => "Holy",
                SpellSchool::Shadow => "Shadow",
                SpellSchool::None => "None",
            };
            
            let message = format!(
                "Team {} {} interrupts Team {} {}'s {} - {} school locked for {:.1}s",
                caster_info.0,
                caster_info.1.name(),
                target_combatant.team,
                target_combatant.class.name(),
                interrupted_spell_name,
                school_name,
                interrupt.lockout_duration
            );
            combat_log.log(CombatLogEventType::AbilityUsed, message);
            
            info!(
                "Team {} {} interrupted! {} school locked for {:.1}s",
                target_combatant.team,
                target_combatant.class.name(),
                school_name,
                interrupt.lockout_duration
            );
        }
        
        // Despawn the interrupt entity
        commands.entity(interrupt_entity).despawn();
    }
}

/// Process casting: update cast timers and apply effects when casts complete.
///
/// When a cast completes:
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
        
        // Handle interrupted casts
        if casting.interrupted {
            // Tick down the interrupted display timer
            casting.interrupted_display_time -= dt;
            
            // Remove CastingState once display time expires
            if casting.interrupted_display_time <= 0.0 {
                commands.entity(caster_entity).remove::<CastingState>();
            }
            
            // Don't process interrupted casts
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
            
            // Pre-calculate damage/healing (using caster's stats)
            let ability_damage = caster.calculate_ability_damage(&def);
            let ability_healing = caster.calculate_ability_healing(&def);
            
            // Store cast info for processing
            completed_casts.push((
                caster_entity,
                caster.team,
                caster.class,
                caster_transform.translation,
                ability_damage,
                ability_healing,
                ability,
                target_entity,
            ));
            
            // Remove casting state
            // Note: GCD was already triggered when the cast began, not here
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
    for (caster_entity, caster_team, caster_class, caster_pos, ability_damage, ability_healing, ability, target_entity) in completed_casts {
        let def = ability.definition();
        
        // Get target
        let Some(target_entity) = target_entity else {
            continue;
        };
        
        // If this ability uses a projectile, spawn it and skip immediate effect application
        if let Some(projectile_speed) = def.projectile_speed {
            // Spawn projectile visual and logic entity
            commands.spawn((
                Projectile {
                    caster: caster_entity,
                    target: target_entity,
                    ability,
                    speed: projectile_speed,
                    caster_team,
                    caster_class,
                },
                PlayMatchEntity,
            ));
            continue; // Skip immediate damage/healing - projectile will handle it on impact
        }
        
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
            // Use pre-calculated damage (already includes stat scaling)
            let damage = ability_damage;
            
            let actual_damage = damage.min(target.current_health);
            target.current_health = (target.current_health - damage).max(0.0);
            target.damage_taken += actual_damage;
            
            // Warriors generate Rage from taking damage
            if target.resource_type == ResourceType::Rage {
                let rage_gain = actual_damage * 0.15; // Gain 15% of damage taken as Rage
                target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
            }
            
            // Track damage for aura breaking
            commands.entity(target_entity).insert(DamageTakenThisFrame {
                amount: actual_damage,
            });
            
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
            
            // Spawn visual effect for Mind Blast (shadow impact)
            if ability == AbilityType::MindBlast {
                commands.spawn((
                    SpellImpactEffect {
                        position: target_pos,
                        lifetime: 0.5,
                        initial_lifetime: 0.5,
                        initial_scale: 0.5,
                        final_scale: 2.0,
                    },
                    PlayMatchEntity,
                ));
            }
            
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
            // Use pre-calculated healing (already includes stat scaling)
            let healing = ability_healing;
            
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
        if let Some((aura_type, duration, magnitude, break_threshold)) = def.applies_aura {
            // We'll apply auras in a separate pass to avoid borrow issues
            commands.spawn((
                AuraPending {
                    target: target_entity,
                    aura: Aura {
                        effect_type: aura_type,
                        duration,
                        magnitude,
                        break_on_damage_threshold: break_threshold,
                        accumulated_damage: 0.0,
                        tick_interval: 0.0,
                        time_until_next_tick: 0.0,
                        caster: Some(caster_entity),
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
    mut combat_log: ResMut<CombatLog>,
    pending_auras: Query<(Entity, &AuraPending)>,
    mut combatants: Query<(&mut Combatant, Option<&mut ActiveAuras>)>,
) {
    for (pending_entity, pending) in pending_auras.iter() {
        // Get target combatant
        let Ok((mut target_combatant, active_auras)) = combatants.get_mut(pending.target) else {
            commands.entity(pending_entity).despawn();
            continue;
        };
        
        // Handle MaxHealthIncrease aura - apply HP buff immediately
        if pending.aura.effect_type == AuraType::MaxHealthIncrease {
            let hp_bonus = pending.aura.magnitude;
            target_combatant.max_health += hp_bonus;
            target_combatant.current_health += hp_bonus; // Give them the extra HP
            
            info!(
                "Team {} {} receives Power Word: Fortitude (+{:.0} max HP, now {:.0}/{:.0})",
                target_combatant.team,
                target_combatant.class.name(),
                hp_bonus,
                target_combatant.current_health,
                target_combatant.max_health
            );
            
            // Log to combat log
            combat_log.log(
                CombatLogEventType::Buff,
                format!(
                    "Team {} {} gains Power Word: Fortitude (+{:.0} max HP)",
                    target_combatant.team,
                    target_combatant.class.name(),
                    hp_bonus
                )
            );
        }
        // Try to get existing auras on target
        if let Some(mut active_auras) = active_auras {
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

/// Process damage-based aura breaking.
/// 
/// When a combatant takes damage, accumulate it on their breakable auras.
/// If accumulated damage exceeds the break threshold, remove the aura.
pub fn process_aura_breaks(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut combatants: Query<(Entity, &Combatant, &mut ActiveAuras, Option<&DamageTakenThisFrame>)>,
) {
    for (entity, combatant, mut active_auras, damage_taken) in combatants.iter_mut() {
        let Some(damage_taken) = damage_taken else {
            continue; // No damage this frame
        };
        
        if damage_taken.amount <= 0.0 {
            continue;
        }
        
        // Track which auras to remove
        let mut auras_to_remove = Vec::new();
        
        // Accumulate damage on breakable auras
        for (index, aura) in active_auras.auras.iter_mut().enumerate() {
            if aura.break_on_damage_threshold > 0.0 {
                aura.accumulated_damage += damage_taken.amount;
                
                // Check if aura should break
                if aura.accumulated_damage >= aura.break_on_damage_threshold {
                    auras_to_remove.push(index);
                    
                    // Log the break
                    let aura_name = match aura.effect_type {
                        AuraType::Root => "Root",
                        AuraType::MovementSpeedSlow => "Movement Speed Slow",
                        AuraType::Stun => "Stun",
                        AuraType::MaxHealthIncrease => "Power Word: Fortitude", // Should never break on damage
                        AuraType::DamageOverTime => "Rend", // Should never break on damage (has 0.0 threshold)
                        AuraType::SpellSchoolLockout => "Lockout", // Should never break on damage (has 0.0 threshold)
                    };
                    
                    let message = format!(
                        "Team {} {}'s {} broke from damage ({:.0}/{:.0})",
                        combatant.team,
                        combatant.class.name(),
                        aura_name,
                        aura.accumulated_damage,
                        aura.break_on_damage_threshold
                    );
                    combat_log.log(CombatLogEventType::MatchEvent, message);
                }
            }
        }
        
        // Remove broken auras (in reverse order to preserve indices)
        for &index in auras_to_remove.iter().rev() {
            active_auras.auras.remove(index);
        }
        
        // Clear damage taken component
        commands.entity(entity).remove::<DamageTakenThisFrame>();
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
    combatants: Query<(Entity, &Combatant, &Transform)>,
    config: Res<MatchConfig>,
    combat_log: Res<CombatLog>,
    celebration: Option<Res<VictoryCelebration>>,
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
/// - Victory text is displayed
/// - Timer counts down
/// 
/// When timer reaches 0:
/// - Transition to Results scene
pub fn update_victory_celebration(
    time: Res<Time>,
    mut celebration: Option<ResMut<VictoryCelebration>>,
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

/// Render victory celebration text.
/// 
/// Displays:
/// - "TEAM X WINS!" or "DRAW!" in large text
/// - Victory message below
pub fn render_victory_celebration(
    mut contexts: EguiContexts,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Only render if celebration is active
    let Some(celebration) = celebration else {
        return;
    };
    
    let ctx = contexts.ctx_mut();
    
    // Display victory message in center of screen
    egui::Area::new(egui::Id::new("victory_celebration"))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, -80.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                // Victory text based on winner
                let (victory_text, victory_color) = match celebration.winner {
                    Some(1) => ("TEAM 1 WINS!", egui::Color32::from_rgb(100, 150, 255)), // Blue
                    Some(2) => ("TEAM 2 WINS!", egui::Color32::from_rgb(255, 100, 100)), // Red
                    None => ("DRAW!", egui::Color32::from_rgb(200, 200, 100)),           // Yellow
                    _ => ("MATCH OVER", egui::Color32::from_rgb(200, 200, 200)),        // Gray
                };
                
                // Large victory text
                ui.label(
                    egui::RichText::new(victory_text)
                        .size(96.0)
                        .color(victory_color)
                        .strong()
                );
                
                ui.add_space(15.0);
                
                // Celebration message (only show "Victory!" if not a draw)
                if celebration.winner.is_some() {
                    ui.label(
                        egui::RichText::new("Victory!")
                            .size(42.0)
                            .color(egui::Color32::from_rgb(255, 215, 0)) // Gold
                    );
                    ui.add_space(10.0);
                }
                
                // Countdown to results
                let seconds_remaining = celebration.time_remaining.ceil() as i32;
                ui.label(
                    egui::RichText::new(format!("Results in {}...", seconds_remaining))
                        .size(20.0)
                        .color(egui::Color32::from_rgb(180, 180, 180))
                );
            });
        });
}

/// Spawn visual meshes for newly created projectiles.
/// Projectiles are represented as glowing ice-blue spheres with a trail effect.
pub fn spawn_projectile_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_projectiles: Query<(Entity, &Projectile), (Added<Projectile>, Without<Mesh3d>)>,
    combatants: Query<&Transform, With<Combatant>>,
) {
    for (projectile_entity, projectile) in new_projectiles.iter() {
        // Get caster position to spawn projectile at that location
        let Ok(caster_transform) = combatants.get(projectile.caster) else {
            continue;
        };
        
        let caster_pos = caster_transform.translation;
        
        // Create a small sphere mesh for the projectile
        let mesh = meshes.add(Sphere::new(0.3));
        
        // Ice blue color with emissive glow
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(0.4, 0.7, 1.0), // Ice blue
            emissive: LinearRgba::rgb(0.6, 0.9, 1.5), // Bright ice glow
            ..default()
        });
        
        // Add visual mesh to the projectile entity
        commands.entity(projectile_entity).insert((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(caster_pos + Vec3::new(0.0, 1.5, 0.0)), // Start at chest height
        ));
    }
}

/// Move projectiles towards their targets.
/// Projectiles travel in a straight line at their defined speed.
pub fn move_projectiles(
    time: Res<Time>,
    mut projectiles: Query<(&Projectile, &mut Transform)>,
    targets: Query<&Transform, (With<Combatant>, Without<Projectile>)>,
) {
    let dt = time.delta_secs();
    
    for (projectile, mut projectile_transform) in projectiles.iter_mut() {
        // Get target position
        let Ok(target_transform) = targets.get(projectile.target) else {
            continue; // Target no longer exists
        };
        
        let target_pos = target_transform.translation + Vec3::new(0.0, 1.0, 0.0); // Aim at center mass
        let current_pos = projectile_transform.translation;
        
        // Calculate direction to target
        let direction = (target_pos - current_pos).normalize_or_zero();
        
        if direction != Vec3::ZERO {
            // Move towards target
            let move_distance = projectile.speed * dt;
            projectile_transform.translation += direction * move_distance;
            
            // Rotate to face direction of travel
            let target_rotation = Quat::from_rotation_arc(Vec3::Z, direction);
            projectile_transform.rotation = target_rotation;
        }
    }
}

/// Check if projectiles have reached their targets and apply effects.
/// When a projectile gets close enough to its target, it "hits" and applies damage/healing/auras.
pub fn process_projectile_hits(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    projectiles: Query<(Entity, &Projectile, &Transform)>,
    mut combatants: Query<(&Transform, &mut Combatant)>,
) {
    const HIT_DISTANCE: f32 = 0.5; // Projectile hits when within 0.5 units of target
    
    // Collect hits to process (to avoid borrow checker issues)
    // Format: (projectile_entity, caster_entity, target_entity, ability, caster_team, caster_class, caster_pos, target_pos, ability_damage, ability_healing)
    let mut hits_to_process: Vec<(Entity, Entity, Entity, AbilityType, u8, match_config::CharacterClass, Vec3, Vec3, f32, f32)> = Vec::new();
    
    for (projectile_entity, projectile, projectile_transform) in projectiles.iter() {
        // Get target position (immutable borrow)
        let Ok((target_transform, target)) = combatants.get(projectile.target) else {
            // Target no longer exists, despawn projectile
            commands.entity(projectile_entity).despawn_recursive();
            continue;
        };
        
        if !target.is_alive() {
            // Target already dead, despawn projectile
            commands.entity(projectile_entity).despawn_recursive();
            continue;
        }
        
        let target_pos = target_transform.translation + Vec3::new(0.0, 1.0, 0.0); // Center mass
        let projectile_pos = projectile_transform.translation;
        let distance = projectile_pos.distance(target_pos);
        
        // Check if projectile has reached target
        if distance <= HIT_DISTANCE {
            // Get caster position (immutable borrow)
            let Ok((caster_transform, _)) = combatants.get(projectile.caster) else {
                // Caster no longer exists, despawn projectile
                commands.entity(projectile_entity).despawn_recursive();
                continue;
            };
            
            let caster_pos = caster_transform.translation;
            let target_world_pos = target_transform.translation;
            
            // Get caster's combatant to calculate damage/healing with stats
            let Ok((_, caster_combatant)) = combatants.get(projectile.caster) else {
                commands.entity(projectile_entity).despawn_recursive();
                continue;
            };
            
            let def = projectile.ability.definition();
            let ability_damage = caster_combatant.calculate_ability_damage(&def);
            let ability_healing = caster_combatant.calculate_ability_healing(&def);
            
            // Queue this hit for processing
            hits_to_process.push((
                projectile_entity,
                projectile.caster,
                projectile.target,
                projectile.ability,
                projectile.caster_team,
                projectile.caster_class,
                caster_pos,
                target_world_pos,
                ability_damage,
                ability_healing,
            ));
        }
    }
    
    // Process all queued hits
    for (projectile_entity, caster_entity, target_entity, ability, caster_team, caster_class, caster_pos, target_pos, ability_damage, ability_healing) in hits_to_process {
        let def = ability.definition();
        let text_position = target_pos + Vec3::new(0.0, 2.0, 0.0);
        let ability_range = caster_pos.distance(target_pos);
        
        // Apply damage
        if def.is_damage() {
            // Use pre-calculated damage (already includes stat scaling)
            let damage = ability_damage;
            
            // Get target info and apply damage
            let (actual_damage, target_team, target_class_name, is_warrior_target) = {
                let Ok((_, mut target)) = combatants.get_mut(target_entity) else {
                    commands.entity(projectile_entity).despawn_recursive();
                    continue;
                };
                
                let actual_damage = damage.min(target.current_health);
                target.current_health = (target.current_health - damage).max(0.0);
                target.damage_taken += actual_damage;
                
                // Warriors generate Rage from taking damage
                if target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15;
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }
                
                // Track damage for aura breaking
                commands.entity(target_entity).insert(DamageTakenThisFrame {
                    amount: actual_damage,
                });
                
                (actual_damage, target.team, target.class.name().to_string(), target.resource_type == ResourceType::Rage)
            }; // target borrow dropped here
            
            // Update caster damage dealt
            {
                let Ok((_, mut caster)) = combatants.get_mut(caster_entity) else {
                    commands.entity(projectile_entity).despawn_recursive();
                    continue;
                };
                caster.damage_dealt += actual_damage;
            } // caster borrow dropped here
            
            // Spawn yellow floating combat text for ability damage
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position,
                    text: format!("{:.0}", actual_damage),
                    color: egui::Color32::from_rgb(255, 255, 0), // Yellow
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
                target_team,
                target_class_name,
                actual_damage
            );
            combat_log.log_with_position(
                CombatLogEventType::Damage,
                message,
                PositionData {
                    entities: vec![
                        format!("Team {} {} (caster)", caster_team, caster_class.name()),
                        format!("Team {} {} (target)", target_team, target_class_name),
                    ],
                    positions: vec![
                        (caster_pos.x, caster_pos.y, caster_pos.z),
                        (target_pos.x, target_pos.y, target_pos.z),
                    ],
                    distance: Some(ability_range),
                },
            );
            
            // Apply aura if ability has one
            if let Some((aura_type, duration, magnitude, break_threshold)) = def.applies_aura {
                commands.spawn(AuraPending {
                    target: target_entity,
                    aura: Aura {
                        effect_type: aura_type,
                        duration,
                        magnitude,
                        break_on_damage_threshold: break_threshold,
                        accumulated_damage: 0.0,
                        tick_interval: if aura_type == AuraType::DamageOverTime { 3.0 } else { 0.0 },
                        time_until_next_tick: if aura_type == AuraType::DamageOverTime { 3.0 } else { 0.0 },
                        caster: Some(caster_entity),
                    },
                });
            }
        }
        
        // Despawn the projectile
        commands.entity(projectile_entity).despawn_recursive();
    }
}

/// Process damage-over-time ticks.
/// 
/// For each combatant with DoT auras:
/// 1. Tick down time_until_next_tick
/// 2. When it reaches 0, apply damage
/// 3. Reset timer for next tick
/// 4. Spawn floating combat text
/// 5. Log to combat log
pub fn process_dot_ticks(
    time: Res<Time>,
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut combatants_with_auras: Query<(Entity, &mut Combatant, &Transform, &mut ActiveAuras)>,
    combatants_without_auras: Query<(Entity, &Combatant), Without<ActiveAuras>>,
) {
    let dt = time.delta_secs();
    
    // Build a map of entity -> (team, class) for quick lookups
    // Include BOTH combatants with auras AND combatants without auras (like the Warrior caster)
    let mut combatant_info: std::collections::HashMap<Entity, (u8, match_config::CharacterClass)> = 
        combatants_with_auras
            .iter()
            .map(|(entity, combatant, _, _)| (entity, (combatant.team, combatant.class)))
            .collect();
    
    // Add combatants without auras to the map
    for (entity, combatant) in combatants_without_auras.iter() {
        combatant_info.insert(entity, (combatant.team, combatant.class));
    }
    
    // Build a map of entity -> position
    let positions: std::collections::HashMap<Entity, Vec3> = combatants_with_auras
        .iter()
        .map(|(entity, _, transform, _)| (entity, transform.translation))
        .collect();
    
    // Track DoT damage to apply (to avoid borrow issues)
    // Format: (target_entity, caster_entity, damage, target_pos, caster_team, caster_class)
    let mut dot_damage_to_apply: Vec<(Entity, Entity, f32, Vec3, u8, match_config::CharacterClass)> = Vec::new();
    
    // First pass: tick down DoT timers and queue damage
    for (entity, combatant, _transform, mut active_auras) in combatants_with_auras.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }
        
        let target_pos = positions.get(&entity).copied().unwrap_or(Vec3::ZERO);
        
        for aura in active_auras.auras.iter_mut() {
            if aura.effect_type != AuraType::DamageOverTime {
                continue;
            }
            
            // Tick down time until next damage application
            // (Duration is already handled by update_auras system)
            aura.time_until_next_tick -= dt;
            
            if aura.time_until_next_tick <= 0.0 {
                // Time to apply DoT damage!
                let damage = aura.magnitude;
                
                // Get caster info (if still exists)
                if let Some(caster_entity) = aura.caster {
                    if let Some(&(caster_team, caster_class)) = combatant_info.get(&caster_entity) {
                        dot_damage_to_apply.push((
                            entity,
                            caster_entity,
                            damage,
                            target_pos,
                            caster_team,
                            caster_class,
                        ));
                    }
                }
                
                // Reset tick timer
                aura.time_until_next_tick = aura.tick_interval;
            }
        }
    }
    
    // Track caster damage dealt updates
    let mut caster_damage_updates: Vec<(Entity, f32)> = Vec::new();
    
    // Second pass: apply queued DoT damage to targets
    for (target_entity, caster_entity, damage, target_pos, caster_team, caster_class) in dot_damage_to_apply {
        // Get target combatant
        let Ok((_, mut target, _, _)) = combatants_with_auras.get_mut(target_entity) else {
            continue;
        };
        
        if !target.is_alive() {
            continue;
        }
        
        let target_team = target.team;
        let target_class = target.class;
        
        // Apply damage
        let actual_damage = damage.min(target.current_health);
        target.current_health = (target.current_health - damage).max(0.0);
        target.damage_taken += actual_damage;
        
        // Track damage for aura breaking
        commands.entity(target_entity).insert(DamageTakenThisFrame {
            amount: actual_damage,
        });
        
        // Warriors generate Rage from taking damage
        if target.resource_type == ResourceType::Rage {
            let rage_gain = actual_damage * 0.15;
            target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
        }
        
        // Queue caster damage_dealt update
        caster_damage_updates.push((caster_entity, actual_damage));
        
        // Spawn floating combat text (yellow for DoT ticks, like ability damage)
        commands.spawn((
            FloatingCombatText {
                world_position: target_pos + Vec3::new(0.0, 2.0, 0.0),
                text: format!("{:.0}", actual_damage),
                color: egui::Color32::from_rgb(255, 255, 0), // Yellow for ability damage
                lifetime: 1.5,
                vertical_offset: 0.0,
            },
            PlayMatchEntity,
        ));
        
        // Log to combat log
        combat_log.log(
            CombatLogEventType::Damage,
            format!(
                "Team {} {}'s Rend ticks for {:.0} damage on Team {} {}",
                caster_team,
                caster_class.name(),
                actual_damage,
                target_team,
                target_class.name()
            ),
        );
    }
    
    // Third pass: update caster damage_dealt stats
    for (caster_entity, damage_dealt) in caster_damage_updates {
        if let Ok((_, mut caster, _, _)) = combatants_with_auras.get_mut(caster_entity) {
            caster.damage_dealt += damage_dealt;
        }
    }
}

