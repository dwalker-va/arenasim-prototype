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
use super::match_config::{self, RogueOpener, WarlockCurse};
use super::abilities::{AbilityType, ScalingStat};
use super::ability_config::AbilityConfig;

use super::constants::{DR_RESET_TIMER, DR_IMMUNE_LEVEL, DR_MULTIPLIERS};

// ============================================================================
// Pet Types
// ============================================================================

/// Pet type enum (extensible for future demons and hunter pets)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PetType {
    Felhunter,
    Spider,
    Boar,
    Bird,
}

impl PetType {
    pub fn name(&self) -> &'static str {
        match self {
            PetType::Felhunter => "Felhunter",
            PetType::Spider => "Spider",
            PetType::Boar => "Boar",
            PetType::Bird => "Bird",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            PetType::Felhunter => Color::srgb(0.4, 0.75, 0.4), // Green (demon)
            PetType::Spider => Color::srgb(0.5, 0.4, 0.3),     // Brown
            PetType::Boar => Color::srgb(0.6, 0.4, 0.3),       // Dark brown
            PetType::Bird => Color::srgb(0.6, 0.7, 0.8),       // Light grey-blue
        }
    }

    pub fn preferred_range(&self) -> f32 {
        match self {
            PetType::Felhunter => 2.0, // Melee
            PetType::Spider => 2.0,    // Melee
            PetType::Boar => 2.0,      // Melee
            PetType::Bird => 2.0,      // Melee
        }
    }

    pub fn movement_speed(&self) -> f32 {
        match self {
            PetType::Felhunter => 5.5,
            PetType::Spider => 5.5,
            PetType::Boar => 6.0, // Slightly faster — aggressive
            PetType::Bird => 5.5,
        }
    }

    pub fn is_melee(&self) -> bool {
        match self {
            PetType::Felhunter => true,
            PetType::Spider => true,
            PetType::Boar => true,
            PetType::Bird => true,
        }
    }

}

/// Marker component for pet entities. Links pet to its owner.
#[derive(Component, Clone)]
pub struct Pet {
    pub owner: Entity,
    pub pet_type: PetType,
}

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
    /// Reduces outgoing physical damage by a percentage (magnitude = 0.2 means 20% reduction)
    /// Used by Curse of Weakness to reduce enemy physical damage dealt.
    DamageReduction,
    /// Increases cast time by a percentage (magnitude = multiplier, e.g., 0.5 = 50% slower)
    /// Used by Curse of Tongues to slow enemy casting.
    CastTimeIncrease,
    /// Reduces incoming damage taken by a percentage (magnitude = 0.10 means 10% reduction)
    /// Used by Devotion Aura to reduce all damage taken by the target.
    DamageTakenReduction,
    /// Complete damage immunity - all incoming damage is negated, all hostile auras are blocked.
    /// Used by Divine Shield. Magnitude unused (always 1.0 by convention).
    DamageImmunity,
    /// Incapacitate - target is frozen in place, can't attack/cast, breaks on ANY damage.
    /// Unlike Polymorph (target wanders), incapacitated targets stand still.
    /// Shares DRCategory::Incapacitates with Polymorph.
    /// Used by Freezing Trap.
    Incapacitate,
}

// ============================================================================
// Diminishing Returns
// ============================================================================

/// DR categories — fixed enum with known size for array indexing.
/// Each category is independent: Stun DR doesn't affect Fear DR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DRCategory {
    Stuns = 0,
    Fears = 1,
    Incapacitates = 2,
    Roots = 3,
    Slows = 4,
}

impl DRCategory {
    pub const COUNT: usize = 5;

    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    /// Map an AuraType to its DR category. Returns None for non-CC auras.
    pub fn from_aura_type(aura_type: &AuraType) -> Option<DRCategory> {
        match aura_type {
            AuraType::Stun => Some(DRCategory::Stuns),
            AuraType::Fear => Some(DRCategory::Fears),
            AuraType::Polymorph | AuraType::Incapacitate => Some(DRCategory::Incapacitates),
            AuraType::Root => Some(DRCategory::Roots),
            AuraType::MovementSpeedSlow => Some(DRCategory::Slows),
            _ => None,
        }
    }
}

/// Per-category DR state. Tracks diminishment level and reset timer.
#[derive(Debug, Clone, Copy, Default)]
pub struct DRState {
    /// 0 = fresh, 1 = next will be 50%, 2 = next will be 25%, 3 = immune
    level: u8,
    /// Seconds remaining until DR resets (counts down from 15.0)
    timer: f32,
}

/// Fixed-size DR tracker component. No heap allocation, fully inline in archetype table.
/// Uses [DRState; 5] indexed by DRCategory discriminant — O(1) access.
#[derive(Component, Debug, Clone)]
pub struct DRTracker {
    states: [DRState; DRCategory::COUNT],
}

impl Default for DRTracker {
    fn default() -> Self {
        Self {
            states: [DRState::default(); DRCategory::COUNT],
        }
    }
}

impl DRTracker {
    /// Apply a CC of the given category. Returns the duration multiplier (1.0, 0.5, 0.25, or 0.0).
    /// Advances DR level and resets the 15s timer (unless already immune).
    #[inline]
    pub fn apply(&mut self, category: DRCategory) -> f32 {
        let state = &mut self.states[category.index()];
        let multiplier = DR_MULTIPLIERS[state.level.min(3) as usize];
        if state.level < DR_IMMUNE_LEVEL {
            state.level += 1;
            state.timer = DR_RESET_TIMER;
        }
        // Immune applications do NOT restart the timer (decision #2)
        multiplier
    }

    /// Check if target is immune to a DR category (level >= 3).
    #[inline]
    pub fn is_immune(&self, category: DRCategory) -> bool {
        self.states[category.index()].level >= DR_IMMUNE_LEVEL
    }

    /// Tick all DR timers. Called from update_auras() each frame.
    pub fn tick_timers(&mut self, dt: f32) {
        for state in &mut self.states {
            if state.timer > 0.0 {
                state.timer -= dt;
                if state.timer <= 0.0 {
                    state.level = 0;
                    state.timer = 0.0;
                }
            }
        }
    }

    /// Get current DR level for a category (for combat log / AI queries).
    #[inline]
    pub fn level(&self, category: DRCategory) -> u8 {
        self.states[category.index()].level
    }
}

// ============================================================================
// Combat Components
// ============================================================================

/// Core combatant component containing all combat state and stats.
#[derive(Component, Clone)]
pub struct Combatant {
    /// Team identifier (1 or 2)
    pub team: u8,
    /// Slot index within the team (0, 1, 2) - used for per-slot configuration like curse targets
    pub slot: u8,
    /// Character class
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
    /// Critical strike chance (0.0 = 0%, 1.0 = 100%). Determines probability of
    /// dealing bonus damage/healing on direct abilities and auto-attacks.
    pub crit_chance: f32,
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
    /// Whether this combatant has died (prevents duplicate death processing)
    pub is_dead: bool,
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
    /// Rogue-specific: which opener to use from stealth (Ambush or Cheap Shot)
    pub rogue_opener: RogueOpener,
    /// Warlock-specific: which curse to apply to each enemy target (indexed by enemy slot)
    pub warlock_curse_prefs: Vec<WarlockCurse>,
}

impl Combatant {
    /// Create a new combatant with class-specific stats.
    pub fn new(team: u8, slot: u8, class: match_config::CharacterClass) -> Self {
        // Class-specific stats (resource_type, health, max_resource, resource_regen, starting_resource, damage, attack speed, attack_power, spell_power, crit_chance, movement speed)
        let (resource_type, max_health, max_resource, resource_regen, starting_resource, attack_damage, attack_speed, attack_power, spell_power, crit_chance, movement_speed) = match class {
            // Warriors: High HP, physical damage, scales with Attack Power (8% crit)
            match_config::CharacterClass::Warrior => (ResourceType::Rage, 200.0, 100.0, 0.0, 0.0, 12.0, 1.0, 30.0, 0.0, 0.08, 5.0),
            // Mages: Low HP, magical damage (wand), scales with Spell Power (6% crit)
            match_config::CharacterClass::Mage => (ResourceType::Mana, 150.0, 200.0, 0.0, 200.0, 10.0, 0.7, 0.0, 50.0, 0.06, 4.5),
            // Rogues: Medium HP, physical burst damage, scales with Attack Power (10% crit - highest)
            match_config::CharacterClass::Rogue => (ResourceType::Energy, 175.0, 100.0, 20.0, 100.0, 10.0, 1.3, 35.0, 0.0, 0.10, 6.0),
            // Priests: Medium HP, healing & wand damage, scales with Spell Power (4% crit)
            match_config::CharacterClass::Priest => (ResourceType::Mana, 150.0, 150.0, 0.0, 150.0, 6.0, 0.8, 0.0, 40.0, 0.04, 5.0),
            // Warlocks: Medium HP, shadow damage (wand), scales with Spell Power, DoT focused (5% crit)
            match_config::CharacterClass::Warlock => (ResourceType::Mana, 160.0, 180.0, 0.0, 180.0, 8.0, 0.7, 0.0, 45.0, 0.05, 4.5),
            // Paladins: High HP (plate), healing & melee hybrid, scales with Spell Power primarily (6% crit)
            // Tankier than Priest but lower spell power to offset utility
            match_config::CharacterClass::Paladin => (ResourceType::Mana, 175.0, 160.0, 0.0, 160.0, 8.0, 0.9, 20.0, 35.0, 0.06, 5.0),
            // Hunters: Medium HP (mail), ranged physical, scales with Attack Power (7% crit)
            // Auto Shot is the primary sustained damage (~18 per 2.5s = 7.2 DPS base).
            match_config::CharacterClass::Hunter => (ResourceType::Mana, 165.0, 150.0, 0.0, 150.0, 18.0, 0.4, 30.0, 0.0, 0.07, 5.0),
        };
        
        // Rogues start stealthed
        let stealthed = class == match_config::CharacterClass::Rogue;
        
        Self {
            team,
            slot,
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
            crit_chance,
            base_movement_speed: movement_speed,
            target: None,
            cc_target: None,
            damage_dealt: 0.0,
            damage_taken: 0.0,
            healing_done: 0.0,
            next_attack_bonus_damage: 0.0,
            is_dead: false,
            stealthed,
            original_color: Color::WHITE, // Will be set correctly when spawning the visual mesh
            ability_cooldowns: std::collections::HashMap::new(),
            global_cooldown: 0.0,
            kiting_timer: 0.0,
            rogue_opener: RogueOpener::default(),
            warlock_curse_prefs: Vec::new(),
        }
    }

    /// Create a new combatant with specific preferences for Warlock curses and rogue opener.
    pub fn new_with_curse_prefs(
        team: u8,
        slot: u8,
        class: match_config::CharacterClass,
        rogue_opener: RogueOpener,
        warlock_curse_prefs: Vec<WarlockCurse>,
    ) -> Self {
        let mut combatant = Self::new(team, slot, class);
        combatant.rogue_opener = rogue_opener;
        combatant.warlock_curse_prefs = warlock_curse_prefs;
        combatant
    }

    /// Create a new pet combatant with stats derived from the owner.
    /// The pet uses its owner's class (Warlock) for combat log identification,
    /// but gets pet-specific stats scaled from the owner.
    pub fn new_pet(team: u8, slot: u8, pet_type: PetType, owner: &Combatant) -> Self {
        match pet_type {
            PetType::Felhunter => {
                let mut pet = Self::new(team, slot, match_config::CharacterClass::Warlock);
                // Scale health to ~45% of owner's max health
                pet.max_health = owner.max_health * 0.45;
                pet.current_health = pet.max_health;
                // Pet-specific stats
                pet.max_mana = 200.0;
                pet.current_mana = 200.0;
                pet.mana_regen = 10.0;
                pet.attack_damage = 8.0;
                pet.attack_speed = 1.2;
                pet.attack_power = 20.0;
                pet.spell_power = owner.spell_power * 0.3;
                pet.crit_chance = 0.05;
                pet.base_movement_speed = pet_type.movement_speed();
                pet
            }
            PetType::Spider | PetType::Boar | PetType::Bird => {
                let mut pet = Self::new(team, slot, match_config::CharacterClass::Hunter);
                // Scale health to ~45% of owner's max health (consistent with Felhunter)
                pet.max_health = owner.max_health * 0.45;
                pet.current_health = pet.max_health;
                // Hunter pets: melee auto-attackers, no mana needed for auto-attacks
                // Pet special abilities use mana from the pet's pool
                pet.max_mana = 100.0;
                pet.current_mana = 100.0;
                pet.mana_regen = 5.0;
                pet.attack_damage = 7.0;
                pet.attack_speed = 1.3;
                pet.attack_power = owner.attack_power * 0.5;
                pet.spell_power = 0.0;
                pet.crit_chance = 0.05;
                pet.base_movement_speed = pet_type.movement_speed();
                pet
            }
        }
    }
    
    /// Check if this combatant is alive (health > 0 and not marked dead).
    pub fn is_alive(&self) -> bool {
        self.current_health > 0.0 && !self.is_dead
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
    /// Spell school of the ability that created this aura (None = physical)
    /// Used to determine if DoTs can be dispelled (only magic DoTs are dispellable)
    pub spell_school: Option<super::abilities::SpellSchool>,
}

impl AuraType {
    /// Returns true if this aura type is inherently magic-dispellable.
    /// This covers CC effects that are always magical in WoW.
    pub fn is_magic_dispellable(&self) -> bool {
        matches!(
            self,
            AuraType::MovementSpeedSlow
                | AuraType::Root
                | AuraType::Fear
                | AuraType::Polymorph
                | AuraType::Incapacitate
        )
    }
}

impl Aura {
    /// Returns true if this aura can be removed by Dispel Magic.
    /// Magic-dispellable aura types (slows, roots, fear, polymorph) are always dispellable.
    /// DoTs are dispellable only if they have a magic spell school (Corruption, Immolate)
    /// but not if they're physical (Rend).
    pub fn can_be_dispelled(&self) -> bool {
        use super::abilities::SpellSchool;

        // Inherently magic-dispellable aura types
        if self.effect_type.is_magic_dispellable() {
            return true;
        }

        // DoTs are dispellable only if magic school
        if matches!(self.effect_type, AuraType::DamageOverTime) {
            if let Some(school) = self.spell_school {
                // Physical DoTs (Rend) are NOT dispellable
                return school != SpellSchool::Physical;
            }
        }

        false
    }
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
        use super::abilities::SpellSchool;

        let aura_effect = ability_def.applies_aura.as_ref()?;

        // Convert spell school to Option (None for Physical, since physical = not magic-dispellable)
        let spell_school = match ability_def.spell_school {
            SpellSchool::Physical | SpellSchool::None => None,
            school => Some(school),
        };

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
                spell_school,
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
        use super::abilities::SpellSchool;

        let aura_effect = ability_def.applies_aura.as_ref()?;

        // Convert spell school to Option (None for Physical, since physical = not magic-dispellable)
        let spell_school = match ability_def.spell_school {
            SpellSchool::Physical | SpellSchool::None => None,
            school => Some(school),
        };

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
                spell_school,
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
        use super::abilities::SpellSchool;

        let aura_effect = ability_def.applies_aura.as_ref()?;

        // Convert spell school to Option (None for Physical, since physical = not magic-dispellable)
        let spell_school = match ability_def.spell_school {
            SpellSchool::Physical | SpellSchool::None => None,
            school => Some(school),
        };

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
                spell_school,
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
    /// Whether this was a critical strike (renders larger with "!" suffix)
    pub is_crit: bool,
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
    /// Whether this is a damage immunity bubble (Divine Shield) vs absorb shield
    /// Immunity bubbles are larger, brighter gold, and have a pulse animation.
    pub is_immunity: bool,
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

/// Visual effect for healing spells - a translucent column of light at the target.
/// Spawned when a healing spell lands, fades over its lifetime.
#[derive(Component)]
pub struct HealingLightColumn {
    /// The entity being healed (column follows this target)
    pub target: Entity,
    /// The class of the healer (affects color: Priest = white-gold, Paladin = golden)
    pub healer_class: match_config::CharacterClass,
    /// Time remaining before despawn (seconds)
    pub lifetime: f32,
    /// Initial lifetime for fade calculation
    pub initial_lifetime: f32,
}

/// Visual effect for dispel spells - an expanding sphere burst at the target.
/// Spawned when a dispel successfully removes an aura, expands and fades over its lifetime.
#[derive(Component)]
pub struct DispelBurst {
    /// The entity that was dispelled (burst follows this target)
    pub target: Entity,
    /// The class of the dispeller (affects color: Priest = white/silver, Paladin = golden)
    pub caster_class: match_config::CharacterClass,
    /// Time remaining before despawn (seconds)
    pub lifetime: f32,
    /// Initial lifetime for fade calculation
    pub initial_lifetime: f32,
}

// ============================================================================
// Hunter Components
// ============================================================================

/// Trap type enum for Hunter traps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapType {
    /// Freezing Trap — incapacitates first enemy contact (breaks on damage)
    Freezing,
    /// Frost Trap — creates a persistent slow zone on trigger
    Frost,
}

/// Component for Hunter traps placed on the ground.
/// Traps have an arming delay, then trigger on enemy proximity.
#[derive(Component)]
pub struct Trap {
    /// Which type of trap this is
    pub trap_type: TrapType,
    /// Team of the hunter who placed this trap
    pub owner_team: u8,
    /// Entity of the hunter who placed this trap
    pub owner: Entity,
    /// Time remaining before the trap is armed (seconds). 0 = armed.
    pub arm_timer: f32,
    /// Trigger radius — enemies within this distance of the trap trigger it
    pub trigger_radius: f32,
    /// Whether this trap has been triggered (pending despawn)
    pub triggered: bool,
}

/// Component for persistent slow zones created by Frost Trap.
/// Enemies inside the zone receive a refreshing movement speed slow.
#[derive(Component)]
pub struct SlowZone {
    /// Team of the hunter who created this zone
    pub owner_team: u8,
    /// Entity of the hunter who created this zone
    pub owner: Entity,
    /// Radius of the slow zone
    pub radius: f32,
    /// Time remaining before the zone expires (seconds)
    pub duration_remaining: f32,
    /// Slow magnitude (movement speed multiplier, e.g., 0.4 = 60% slow)
    pub slow_magnitude: f32,
}

/// Component tracking an active Disengage (Hunter backward leap).
#[derive(Component)]
pub struct DisengagingState {
    /// Direction of the leap (normalized, away from nearest enemy)
    pub direction: Vec3,
    /// Distance remaining to travel
    pub distance_remaining: f32,
}

// ============================================================================
// Paladin Pending Components
// ============================================================================

/// Pending Holy Shock heal to be processed.
#[derive(Component)]
pub struct HolyShockHealPending {
    pub caster_spell_power: f32,
    pub caster_crit_chance: f32,
    pub caster_team: u8,
    pub caster_class: match_config::CharacterClass,
    pub target: Entity,
}

/// Pending Holy Shock damage to be processed.
#[derive(Component)]
pub struct HolyShockDamagePending {
    pub caster_spell_power: f32,
    pub caster_crit_chance: f32,
    pub caster_team: u8,
    pub caster_class: match_config::CharacterClass,
    pub target: Entity,
}

/// Pending Divine Shield activation to be processed.
/// Uses the deferred pending pattern because Paladin AI has immutable aura access.
/// The process_divine_shield() system has mutable ActiveAuras and can purge debuffs + apply immunity.
#[derive(Component)]
pub struct DivineShieldPending {
    pub caster: Entity,
    pub caster_team: u8,
    pub caster_class: match_config::CharacterClass,
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

    // =========================================================================
    // DRCategory Tests
    // =========================================================================

    #[test]
    fn test_dr_category_from_aura_type_cc_types() {
        assert_eq!(DRCategory::from_aura_type(&AuraType::Stun), Some(DRCategory::Stuns));
        assert_eq!(DRCategory::from_aura_type(&AuraType::Fear), Some(DRCategory::Fears));
        assert_eq!(DRCategory::from_aura_type(&AuraType::Polymorph), Some(DRCategory::Incapacitates));
        assert_eq!(DRCategory::from_aura_type(&AuraType::Root), Some(DRCategory::Roots));
        assert_eq!(DRCategory::from_aura_type(&AuraType::MovementSpeedSlow), Some(DRCategory::Slows));
    }

    #[test]
    fn test_dr_category_from_aura_type_non_cc_returns_none() {
        assert_eq!(DRCategory::from_aura_type(&AuraType::DamageOverTime), None);
        assert_eq!(DRCategory::from_aura_type(&AuraType::MaxHealthIncrease), None);
        assert_eq!(DRCategory::from_aura_type(&AuraType::Absorb), None);
        assert_eq!(DRCategory::from_aura_type(&AuraType::SpellSchoolLockout), None);
        assert_eq!(DRCategory::from_aura_type(&AuraType::HealingReduction), None);
        assert_eq!(DRCategory::from_aura_type(&AuraType::DamageImmunity), None);
    }

    // =========================================================================
    // DRTracker Tests
    // =========================================================================

    #[test]
    fn test_dr_tracker_apply_returns_correct_multipliers() {
        let mut tracker = DRTracker::default();
        // First application: 100% duration
        assert_eq!(tracker.apply(DRCategory::Stuns), 1.0);
        // Second: 50%
        assert_eq!(tracker.apply(DRCategory::Stuns), 0.5);
        // Third: 25%
        assert_eq!(tracker.apply(DRCategory::Stuns), 0.25);
        // Fourth: immune (0%)
        assert_eq!(tracker.apply(DRCategory::Stuns), 0.0);
        // Fifth: still immune
        assert_eq!(tracker.apply(DRCategory::Stuns), 0.0);
    }

    #[test]
    fn test_dr_tracker_categories_are_independent() {
        let mut tracker = DRTracker::default();
        // Advance stun DR to immune
        tracker.apply(DRCategory::Stuns);
        tracker.apply(DRCategory::Stuns);
        tracker.apply(DRCategory::Stuns);
        assert!(tracker.is_immune(DRCategory::Stuns));
        // Fear DR should still be fresh
        assert!(!tracker.is_immune(DRCategory::Fears));
        assert_eq!(tracker.apply(DRCategory::Fears), 1.0);
    }

    #[test]
    fn test_dr_tracker_is_immune() {
        let mut tracker = DRTracker::default();
        assert!(!tracker.is_immune(DRCategory::Roots));
        tracker.apply(DRCategory::Roots); // level 1
        assert!(!tracker.is_immune(DRCategory::Roots));
        tracker.apply(DRCategory::Roots); // level 2
        assert!(!tracker.is_immune(DRCategory::Roots));
        tracker.apply(DRCategory::Roots); // level 3 = immune
        assert!(tracker.is_immune(DRCategory::Roots));
    }

    #[test]
    fn test_dr_tracker_tick_timers_reset() {
        let mut tracker = DRTracker::default();
        tracker.apply(DRCategory::Fears); // level 1, timer = 15.0
        assert_eq!(tracker.level(DRCategory::Fears), 1);

        // Tick 14 seconds — still active
        tracker.tick_timers(14.0);
        assert_eq!(tracker.level(DRCategory::Fears), 1);

        // Tick past 15s — should reset
        tracker.tick_timers(2.0);
        assert_eq!(tracker.level(DRCategory::Fears), 0);
        assert!(!tracker.is_immune(DRCategory::Fears));
    }

    #[test]
    fn test_dr_tracker_immune_apply_does_not_restart_timer() {
        let mut tracker = DRTracker::default();
        // Get to immune
        tracker.apply(DRCategory::Slows);
        tracker.apply(DRCategory::Slows);
        tracker.apply(DRCategory::Slows);
        assert!(tracker.is_immune(DRCategory::Slows));

        // Tick 10 seconds
        tracker.tick_timers(10.0);

        // Apply while immune — should NOT restart the timer
        let mult = tracker.apply(DRCategory::Slows);
        assert_eq!(mult, 0.0);

        // 6 more seconds (total 16s from original apply) — should have reset
        tracker.tick_timers(6.0);
        assert!(!tracker.is_immune(DRCategory::Slows));
        assert_eq!(tracker.level(DRCategory::Slows), 0);
    }
}
