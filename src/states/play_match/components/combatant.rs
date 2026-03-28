use bevy::prelude::*;
use std::collections::HashMap;
use super::super::match_config::{self, RogueOpener, WarlockCurse};
use super::super::abilities::{AbilityType, ScalingStat};
use super::super::ability_config::AbilityConfig;
use super::super::equipment::{ItemSlot, ItemId, ItemDefinitions};
use super::auras::AuraType;
use super::pets::PetType;
use super::resources::GameRng;

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
            match_config::CharacterClass::Hunter => (ResourceType::Mana, 165.0, 150.0, 3.0, 150.0, 18.0, 0.4, 30.0, 0.0, 0.07, 5.0),
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

    /// Apply equipment stats from a resolved loadout to this combatant.
    ///
    /// - Armor/accessory items: ADD their stats to combatant fields.
    /// - Weapon in the primary slot (MainHand for melee, Ranged for ranged): REPLACE
    ///   attack_damage and attack_speed, ADD other stats.
    /// - Off Hand weapons: only ADD non-weapon stats (no attack_damage/attack_speed replacement).
    /// - After all items: reset current_health and current_mana to their new maximums.
    pub fn apply_equipment(&mut self, loadout: &HashMap<ItemSlot, ItemId>, items: &ItemDefinitions) {
        // Determine the primary weapon slot based on class
        let primary_weapon_slot = if self.class.is_melee() {
            ItemSlot::MainHand
        } else {
            ItemSlot::Ranged
        };

        for (slot, item_id) in loadout {
            let Some(item) = items.get(item_id) else {
                continue;
            };

            // Always add general stats from every item
            self.max_health += item.max_health;
            self.max_mana += item.max_mana;
            self.mana_regen += item.mana_regen;
            self.attack_power += item.attack_power;
            self.spell_power += item.spell_power;
            self.crit_chance += item.crit_chance;
            self.base_movement_speed += item.movement_speed;

            // For the primary weapon slot, replace attack_damage and attack_speed
            if item.is_weapon && *slot == primary_weapon_slot {
                let avg_damage = (item.attack_damage_min + item.attack_damage_max) / 2.0;
                self.attack_damage = avg_damage;
                self.attack_speed = item.attack_speed;
            }
            // Off Hand weapons: no attack_damage/attack_speed replacement (stats already added above)
        }

        // Reset current pools to new maximums
        self.current_health = self.max_health;
        self.current_mana = self.max_mana;
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

impl CastingState {
    /// Create a new CastingState with sensible defaults.
    /// Sets `interrupted` to false and `interrupted_display_time` to 0.0.
    pub fn new(ability: AbilityType, target: Entity, cast_time: f32) -> Self {
        Self {
            ability,
            time_remaining: cast_time,
            target: Some(target),
            interrupted: false,
            interrupted_display_time: 0.0,
        }
    }
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

/// Pending dispel to be processed by the aura system.
/// This allows dispels to be applied without holding mutable references
/// to the aura map during AI decision making.
/// Note: The actual aura removed is randomly selected in process_dispels (WoW Classic behavior).
///
/// Used by Priest (Dispel Magic), Paladin (Cleanse), Felhunter (Devour Magic),
/// and Bird (Master's Call).
#[derive(Component)]
pub struct DispelPending {
    /// Target entity to dispel
    pub target: Entity,
    /// Log prefix for combat log (e.g., "[DISPEL]" for Priest, "[CLEANSE]" for Paladin)
    pub log_prefix: &'static str,
    /// Caster's class for visual effect coloring
    pub caster_class: match_config::CharacterClass,
    /// Entity to heal on successful dispel (Felhunter's Devour Magic heals itself)
    pub heal_on_success: Option<(Entity, f32)>,
    /// Optional filter: only remove auras matching these types (Master's Call only removes movement impairments)
    pub aura_type_filter: Option<Vec<AuraType>>,
}
