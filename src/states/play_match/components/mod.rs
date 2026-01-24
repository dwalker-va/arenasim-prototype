//! Component Definitions for Play Match
//!
//! This module contains all ECS components, resources, and data structures
//! used during the match simulation.
//!
//! ## Module Structure
//!
//! Components are organized into logical groups (with documentation submodules):
//! - `auras`: Buff/debuff system (AuraType, Aura, ActiveAuras, AuraPending)
//! - `visual`: Visual effects (FloatingCombatText, ShieldBubble, DeathAnimation)
//!
//! All types are defined here and re-exported for backward compatibility.
//! The submodules exist primarily for documentation organization.

// Documentation submodules - these provide focused documentation
// but types are still defined in this file for simplicity
pub mod auras;
pub mod visual;

use bevy::prelude::*;
use bevy_egui::egui;
use rand::prelude::*;
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use super::match_config;
use super::abilities::{AbilityType, ScalingStat};
use super::ability_config::AbilityConfig;

// Re-export constants from parent module
use super::{MELEE_RANGE, WAND_RANGE};

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

/// Component marking a Shadow Sight orb entity.
/// These orbs spawn after extended combat to break stealth stalemates.
#[derive(Component)]
pub struct ShadowSightOrb {
    /// Which orb spawn point this is (0 or 1)
    pub spawn_index: u8,
}

/// Component marking a Shadow Sight orb that is being consumed.
/// The orb will animate (shrink toward collector, fade) before despawning.
#[derive(Component)]
pub struct ShadowSightOrbConsuming {
    /// Entity that picked up the orb (for movement toward them)
    pub collector: Entity,
    /// Time remaining before despawn (seconds)
    pub lifetime: f32,
    /// Initial lifetime for calculating animation progress
    pub initial_lifetime: f32,
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
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
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
    /// Increases maximum mana by a flat amount (magnitude = mana bonus)
    MaxManaIncrease,
    /// Increases attack power by a flat amount (magnitude = AP bonus)
    AttackPowerIncrease,
    /// Shadow Sight - reveals stealthed enemies AND makes the holder visible to enemies
    ShadowSight,
    /// Absorbs incoming damage (magnitude = remaining absorb amount)
    /// When damage is absorbed, magnitude decreases. Aura removed when magnitude reaches 0.
    Absorb,
    /// Weakened Soul - prevents receiving Power Word: Shield (applied by PW:S)
    WeakenedSoul,
    /// Polymorph - target wanders slowly, can't attack/cast, breaks on ANY damage.
    /// Separate from Stun for diminishing returns categories (incapacitates vs stuns).
    Polymorph,
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
    /// Current target entity for damage (None if no valid target)
    pub target: Option<Entity>,
    /// CC target entity (separate from damage target, None = use heuristics)
    pub cc_target: Option<Entity>,
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
            cc_target: None,
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

    /// Validate that all combatant invariants hold.
    ///
    /// This is useful for debugging - call this after modifying combatant state
    /// to ensure no invariants have been violated.
    ///
    /// In debug builds, this panics on invariant violations.
    /// In release builds, this is a no-op.
    #[inline]
    pub fn debug_validate(&self) {
        debug_assert!(
            self.current_health >= 0.0,
            "Combatant health cannot be negative: {}",
            self.current_health
        );
        debug_assert!(
            self.current_health <= self.max_health,
            "Combatant health ({}) cannot exceed max_health ({})",
            self.current_health,
            self.max_health
        );
        debug_assert!(
            self.current_mana >= 0.0,
            "Combatant mana cannot be negative: {}",
            self.current_mana
        );
        debug_assert!(
            self.max_health > 0.0,
            "Combatant max_health must be positive: {}",
            self.max_health
        );
        debug_assert!(
            self.team == 1 || self.team == 2,
            "Combatant team must be 1 or 2, got {}",
            self.team
        );
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
    ///
    /// Uses the provided GameRng for deterministic results when seeded.
    pub fn calculate_ability_damage_config(&self, ability_config: &AbilityConfig, rng: &mut GameRng) -> f32 {
        // Calculate base damage (random between min and max)
        let damage_range = ability_config.damage_base_max - ability_config.damage_base_min;
        let base_damage = ability_config.damage_base_min + (rng.random_f32() * damage_range);

        // Add stat scaling
        let stat_value = match ability_config.damage_scales_with {
            ScalingStat::AttackPower => self.attack_power,
            ScalingStat::SpellPower => self.spell_power,
            ScalingStat::None => 0.0,
        };

        base_damage + (stat_value * ability_config.damage_coefficient)
    }

    /// Calculate healing for an ability using the new data-driven AbilityConfig.
    /// Formula: Base Healing + (Spell Power × Coefficient)
    ///
    /// Uses the provided GameRng for deterministic results when seeded.
    pub fn calculate_ability_healing_config(&self, ability_config: &AbilityConfig, rng: &mut GameRng) -> f32 {
        // Calculate base healing (random between min and max)
        let healing_range = ability_config.healing_base_max - ability_config.healing_base_min;
        let base_healing = ability_config.healing_base_min + (rng.random_f32() * healing_range);

        // Add spell power scaling (healing always scales with spell power in WoW)
        base_healing + (self.spell_power * ability_config.healing_coefficient)
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

/// Component tracking an active channel in progress.
/// Channeled spells deal their effects over time while the caster remains stationary.
#[derive(Component)]
pub struct ChannelingState {
    /// The ability being channeled
    pub ability: AbilityType,
    /// Total duration remaining for the channel (in seconds)
    pub duration_remaining: f32,
    /// Time until next tick applies (in seconds)
    pub time_until_next_tick: f32,
    /// How often ticks occur (in seconds)
    pub tick_interval: f32,
    /// Target entity receiving the channel effects
    pub target: Entity,
    /// Whether this channel was interrupted (for visual feedback)
    pub interrupted: bool,
    /// Time remaining to show interrupted state (before removing ChannelingState)
    pub interrupted_display_time: f32,
    /// Number of ticks that have been applied
    pub ticks_applied: u32,
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

impl AuraPending {
    /// Create an AuraPending from an ability config.
    ///
    /// This is a helper method that extracts the aura info from an AbilityConfig
    /// and creates an AuraPending with appropriate defaults.
    ///
    /// Returns None if the ability doesn't apply an aura.
    pub fn from_ability(
        target: Entity,
        caster: Entity,
        ability_def: &AbilityConfig,
    ) -> Option<Self> {
        let aura_effect = ability_def.applies_aura.as_ref()?;

        Some(Self {
            target,
            aura: Aura {
                effect_type: aura_effect.aura_type,
                duration: aura_effect.duration,
                magnitude: aura_effect.magnitude,
                break_on_damage_threshold: aura_effect.break_on_damage,
                accumulated_damage: 0.0,
                tick_interval: aura_effect.tick_interval,
                time_until_next_tick: aura_effect.tick_interval,
                caster: Some(caster),
                ability_name: ability_def.name.clone(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
            },
        })
    }

    /// Create an AuraPending for a DoT (Damage over Time) effect.
    ///
    /// DoTs have tick intervals and need special handling for damage attribution.
    pub fn from_ability_dot(
        target: Entity,
        caster: Entity,
        ability_def: &AbilityConfig,
        tick_interval: f32,
    ) -> Option<Self> {
        let aura_effect = ability_def.applies_aura.as_ref()?;

        Some(Self {
            target,
            aura: Aura {
                effect_type: aura_effect.aura_type,
                duration: aura_effect.duration,
                magnitude: aura_effect.magnitude,
                break_on_damage_threshold: aura_effect.break_on_damage,
                accumulated_damage: 0.0,
                tick_interval,
                time_until_next_tick: tick_interval, // First tick after interval
                caster: Some(caster),
                ability_name: ability_def.name.clone(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
            },
        })
    }

    /// Create an AuraPending with a custom ability name override.
    ///
    /// Useful when the display name should differ from the ability definition.
    pub fn from_ability_with_name(
        target: Entity,
        caster: Entity,
        ability_def: &AbilityConfig,
        ability_name: String,
    ) -> Option<Self> {
        let aura_effect = ability_def.applies_aura.as_ref()?;

        Some(Self {
            target,
            aura: Aura {
                effect_type: aura_effect.aura_type,
                duration: aura_effect.duration,
                magnitude: aura_effect.magnitude,
                break_on_damage_threshold: aura_effect.break_on_damage,
                accumulated_damage: 0.0,
                tick_interval: aura_effect.tick_interval,
                time_until_next_tick: aura_effect.tick_interval,
                caster: Some(caster),
                ability_name,
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
            },
        })
    }
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

/// Component for tracking death fall animation.
/// When a combatant dies, this component is added to animate them falling over.
#[derive(Component)]
pub struct DeathAnimation {
    /// Animation progress (0.0 = start, 1.0 = complete)
    pub progress: f32,
    /// Fall direction (normalized, in XZ plane)
    pub fall_direction: Vec3,
}

impl DeathAnimation {
    /// Duration of the death fall animation in seconds
    pub const DURATION: f32 = 0.6;

    pub fn new(fall_direction: Vec3) -> Self {
        Self {
            progress: 0.0,
            fall_direction: fall_direction.normalize(),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.progress >= 1.0
    }
}

/// Component for shield bubble visual effects.
/// Attached to a sphere entity that visually represents an absorb shield around a combatant.
#[derive(Component)]
pub struct ShieldBubble {
    /// The combatant entity this bubble belongs to
    pub combatant: Entity,
    /// The spell school of the shield (affects color: Frost = blue, Holy = gold)
    pub spell_school: super::abilities::SpellSchool,
}

/// Component that stores the original mesh handle for a combatant.
/// Used to restore the mesh when polymorph ends.
#[derive(Component)]
pub struct OriginalMesh(pub Handle<Mesh>);

/// Marker component indicating the combatant is currently polymorphed.
/// Used to track mesh swapping state.
#[derive(Component)]
pub struct PolymorphedVisual;

/// A rising flame particle for fire spell effects (e.g., Immolate).
/// Spawned at target location, rises upward while shrinking and fading.
#[derive(Component)]
pub struct FlameParticle {
    /// Velocity vector (primarily upward with slight horizontal drift)
    pub velocity: Vec3,
    /// Time remaining before despawn (seconds)
    pub lifetime: f32,
    /// Initial lifetime for fade/shrink calculation
    pub initial_lifetime: f32,
}

/// Drain Life beam effect connecting caster to target.
/// Created when a Drain Life channel starts, despawned when it ends.
#[derive(Component)]
pub struct DrainLifeBeam {
    /// The caster entity channeling Drain Life
    pub caster: Entity,
    /// The target entity being drained
    pub target: Entity,
    /// Timer for spawning particles along the beam
    pub particle_spawn_timer: f32,
}

/// A particle flowing along the Drain Life beam from target to caster.
#[derive(Component)]
pub struct DrainParticle {
    /// Progress along beam: 0.0 = at target, 1.0 = at caster
    pub progress: f32,
    /// Movement speed (progress units per second)
    pub speed: f32,
    /// Reference to the beam this particle belongs to
    pub beam: Entity,
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // GameRng Tests
    // =========================================================================

    #[test]
    fn test_seeded_rng_is_deterministic() {
        let seed = 42;
        let mut rng1 = GameRng::from_seed(seed);
        let mut rng2 = GameRng::from_seed(seed);

        // Both RNGs should produce identical sequences
        for _ in 0..100 {
            assert_eq!(rng1.random_f32(), rng2.random_f32());
        }
    }

    #[test]
    fn test_different_seeds_produce_different_results() {
        let mut rng1 = GameRng::from_seed(1);
        let mut rng2 = GameRng::from_seed(2);

        // Different seeds should produce different first values
        assert_ne!(rng1.random_f32(), rng2.random_f32());
    }

    #[test]
    fn test_random_range() {
        let mut rng = GameRng::from_seed(123);

        for _ in 0..100 {
            let value = rng.random_range(10.0, 20.0);
            assert!(value >= 10.0, "Value {} should be >= 10.0", value);
            assert!(value < 20.0, "Value {} should be < 20.0", value);
        }
    }

    #[test]
    fn test_seeded_rng_stores_seed() {
        let seed = 12345;
        let rng = GameRng::from_seed(seed);
        assert_eq!(rng.seed, Some(seed));
    }

    #[test]
    fn test_entropy_rng_has_no_seed() {
        let rng = GameRng::from_entropy();
        assert!(rng.seed.is_none());
    }

    // =========================================================================
    // AuraPending Helper Tests
    // =========================================================================

    #[test]
    fn test_aura_pending_from_ability_with_aura() {
        use super::AbilityType;
        use super::super::ability_config::AbilityDefinitions;

        // Ice Barrier has an absorb aura
        let abilities = AbilityDefinitions::default();
        let ability_def = abilities.get_unchecked(&AbilityType::IceBarrier);
        let target = Entity::from_raw(1);
        let caster = Entity::from_raw(2);

        let pending = AuraPending::from_ability(target, caster, ability_def);

        assert!(pending.is_some(), "Ice Barrier should create an AuraPending");
        let pending = pending.unwrap();
        assert_eq!(pending.target, target);
        assert_eq!(pending.aura.caster, Some(caster));
        assert_eq!(pending.aura.effect_type, AuraType::Absorb);
        assert_eq!(pending.aura.ability_name, "Ice Barrier");
    }

    #[test]
    fn test_aura_pending_from_ability_without_aura() {
        use super::AbilityType;
        use super::super::ability_config::AbilityDefinitions;

        // Shadowbolt doesn't have an aura
        let abilities = AbilityDefinitions::default();
        let ability_def = abilities.get_unchecked(&AbilityType::Shadowbolt);
        let target = Entity::from_raw(1);
        let caster = Entity::from_raw(2);

        let pending = AuraPending::from_ability(target, caster, ability_def);

        assert!(pending.is_none(), "Shadowbolt should not create an AuraPending");
    }

    #[test]
    fn test_aura_pending_dot_has_tick_interval() {
        use super::AbilityType;
        use super::super::ability_config::AbilityDefinitions;

        // Corruption is a DoT
        let abilities = AbilityDefinitions::default();
        let ability_def = abilities.get_unchecked(&AbilityType::Corruption);
        let target = Entity::from_raw(1);
        let caster = Entity::from_raw(2);

        let pending = AuraPending::from_ability_dot(target, caster, ability_def, 3.0);

        assert!(pending.is_some(), "Corruption should create an AuraPending");
        let pending = pending.unwrap();
        assert_eq!(pending.aura.tick_interval, 3.0);
        assert_eq!(pending.aura.time_until_next_tick, 3.0);
        assert_eq!(pending.aura.effect_type, AuraType::DamageOverTime);
    }

    #[test]
    fn test_aura_pending_custom_name() {
        use super::AbilityType;
        use super::super::ability_config::AbilityDefinitions;

        let abilities = AbilityDefinitions::default();
        let ability_def = abilities.get_unchecked(&AbilityType::IceBarrier);
        let target = Entity::from_raw(1);
        let caster = Entity::from_raw(2);

        let pending = AuraPending::from_ability_with_name(
            target,
            caster,
            ability_def,
            "Custom Shield Name".to_string(),
        );

        assert!(pending.is_some());
        let pending = pending.unwrap();
        assert_eq!(pending.aura.ability_name, "Custom Shield Name");
    }
}
