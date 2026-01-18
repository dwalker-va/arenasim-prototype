//! Data-Driven Ability Configuration
//!
//! This module provides data-driven ability definitions loaded from RON config files.
//! Instead of hardcoding ability stats in Rust, abilities are defined in `assets/config/abilities.ron`.
//!
//! ## Benefits
//! - Balance changes don't require recompilation
//! - Easier to review and modify ability values
//! - Validates all abilities exist at startup
//!
//! ## Usage
//! ```ignore
//! fn my_system(abilities: Res<AbilityDefinitions>) {
//!     let def = abilities.get(&AbilityType::Frostbolt).unwrap();
//!     println!("Frostbolt cast time: {}", def.cast_time);
//! }
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::abilities::{AbilityType, ScalingStat, SpellSchool};
use super::components::AuraType;

/// Default value for break_on_damage: -1.0 means the aura doesn't break on damage.
fn default_break_on_damage() -> f32 {
    -1.0
}

/// Aura effect configuration with named fields.
///
/// Replaces the old tuple format `(AuraType, duration, magnitude, break_threshold)`
/// for better readability in config files.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuraEffect {
    /// The type of aura effect to apply
    pub aura_type: AuraType,
    /// Duration of the aura in seconds
    pub duration: f32,
    /// Effect magnitude (meaning depends on aura_type)
    /// - MovementSpeedSlow: multiplier (0.7 = 30% slow)
    /// - Absorb: amount of damage absorbed
    /// - DamageOverTime: damage per tick
    /// - HealingReduction: multiplier (0.65 = 35% reduction)
    pub magnitude: f32,
    /// Damage threshold that breaks the aura.
    /// - Negative (default -1.0) = doesn't break on damage
    /// - 0.0 = breaks on ANY damage (e.g., Polymorph)
    /// - Positive = breaks when accumulated damage exceeds threshold
    #[serde(default = "default_break_on_damage")]
    pub break_on_damage: f32,
    /// Tick interval for DoT effects in seconds (0.0 = no ticks)
    #[serde(default)]
    pub tick_interval: f32,
}

/// Projectile visual configuration.
///
/// Defines the colors for projectile spells.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectileVisuals {
    /// Base RGB color (0.0-1.0 range)
    pub color: [f32; 3],
    /// Emissive/glow RGB color (can exceed 1.0 for glow effect)
    pub emissive: [f32; 3],
}

/// Complete ability configuration loaded from RON.
///
/// This struct mirrors `AbilityDefinition` but with:
/// - Named struct for aura effects instead of tuple
/// - Additional fields for special behavior flags
/// - Projectile visual configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AbilityConfig {
    /// Display name of the ability
    pub name: String,

    // === Casting ===
    /// Cast time in seconds (0.0 = instant)
    #[serde(default)]
    pub cast_time: f32,
    /// Maximum range in units
    pub range: f32,
    /// Resource cost (mana, energy, or rage)
    #[serde(default)]
    pub mana_cost: f32,
    /// Cooldown after cast in seconds
    #[serde(default)]
    pub cooldown: f32,

    // === Damage ===
    /// Base minimum damage (before stat scaling)
    #[serde(default)]
    pub damage_base_min: f32,
    /// Base maximum damage (before stat scaling)
    #[serde(default)]
    pub damage_base_max: f32,
    /// Coefficient for stat scaling: Damage = Base + (Stat * Coefficient)
    #[serde(default)]
    pub damage_coefficient: f32,
    /// Which stat this ability's damage scales with
    #[serde(default = "default_scaling_none")]
    pub damage_scales_with: ScalingStat,

    // === Healing ===
    /// Base minimum healing (before stat scaling)
    #[serde(default)]
    pub healing_base_min: f32,
    /// Base maximum healing (before stat scaling)
    #[serde(default)]
    pub healing_base_max: f32,
    /// Coefficient for spell power scaling: Healing = Base + (SpellPower * Coefficient)
    #[serde(default)]
    pub healing_coefficient: f32,

    // === Effects ===
    /// Aura to apply on hit/cast (if any)
    #[serde(default)]
    pub applies_aura: Option<AuraEffect>,
    /// Projectile travel speed in units/second (None = instant effect)
    #[serde(default)]
    pub projectile_speed: Option<f32>,
    /// Projectile visual colors (if projectile_speed is Some)
    #[serde(default)]
    pub projectile_visuals: Option<ProjectileVisuals>,

    // === Spell School & Interrupts ===
    /// Spell school (determines lockout when interrupted)
    #[serde(default = "default_spell_school_none")]
    pub spell_school: SpellSchool,
    /// Whether this ability interrupts the target's casting
    #[serde(default)]
    pub is_interrupt: bool,
    /// Lockout duration in seconds (for interrupt abilities)
    #[serde(default)]
    pub lockout_duration: f32,

    // === Special Behavior Flags ===
    /// Requires stealth to cast (Ambush)
    #[serde(default)]
    pub requires_stealth: bool,
    /// This is a charge/gap-closer ability (Charge)
    #[serde(default)]
    pub is_charge: bool,
    /// Spawn visual impact effect on hit (Mind Blast)
    #[serde(default)]
    pub spawn_impact_effect: bool,
}

fn default_scaling_none() -> ScalingStat {
    ScalingStat::None
}

fn default_spell_school_none() -> SpellSchool {
    SpellSchool::None
}

impl AbilityConfig {
    /// Returns true if this is a damage ability
    pub fn is_damage(&self) -> bool {
        self.damage_base_max > 0.0 || self.damage_coefficient > 0.0
    }

    /// Returns true if this is a healing ability
    pub fn is_heal(&self) -> bool {
        self.healing_base_max > 0.0 || self.healing_coefficient > 0.0
    }
}

/// Root structure for the abilities.ron file
#[derive(Debug, Serialize, Deserialize)]
pub struct AbilitiesConfig {
    pub abilities: HashMap<AbilityType, AbilityConfig>,
}

/// Resource containing all ability definitions.
///
/// Loaded from `assets/config/abilities.ron` at startup.
/// Access via `Res<AbilityDefinitions>` in systems.
#[derive(Resource)]
pub struct AbilityDefinitions {
    definitions: HashMap<AbilityType, AbilityConfig>,
}

impl Default for AbilityDefinitions {
    /// Load ability definitions from the default config file.
    /// Panics if the file cannot be loaded - use for tests only.
    fn default() -> Self {
        load_ability_definitions()
            .expect("Failed to load ability definitions in Default impl")
    }
}

impl AbilityDefinitions {
    /// Create from a loaded config
    pub fn new(config: AbilitiesConfig) -> Self {
        Self {
            definitions: config.abilities,
        }
    }

    /// Get the configuration for an ability type
    pub fn get(&self, ability: &AbilityType) -> Option<&AbilityConfig> {
        self.definitions.get(ability)
    }

    /// Get the configuration for an ability type, panicking if not found.
    /// Use this when you know the ability must exist (validated at startup).
    pub fn get_unchecked(&self, ability: &AbilityType) -> &AbilityConfig {
        self.definitions.get(ability)
            .unwrap_or_else(|| panic!("Ability {:?} not found in definitions", ability))
    }

    /// Check if all expected ability types are defined
    pub fn validate(&self) -> Result<(), Vec<AbilityType>> {
        let expected_abilities = [
            AbilityType::Frostbolt,
            AbilityType::FlashHeal,
            AbilityType::HeroicStrike,
            AbilityType::Ambush,
            AbilityType::FrostNova,
            AbilityType::MindBlast,
            AbilityType::SinisterStrike,
            AbilityType::Charge,
            AbilityType::KidneyShot,
            AbilityType::PowerWordFortitude,
            AbilityType::Rend,
            AbilityType::MortalStrike,
            AbilityType::Pummel,
            AbilityType::Kick,
            AbilityType::Corruption,
            AbilityType::Shadowbolt,
            AbilityType::Fear,
            AbilityType::ArcaneIntellect,
            AbilityType::BattleShout,
            AbilityType::IceBarrier,
            AbilityType::PowerWordShield,
        ];

        let missing: Vec<AbilityType> = expected_abilities
            .into_iter()
            .filter(|ability| !self.definitions.contains_key(ability))
            .collect();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(missing)
        }
    }

    /// Get all ability types that are defined
    pub fn ability_types(&self) -> impl Iterator<Item = &AbilityType> {
        self.definitions.keys()
    }
}

/// Load ability definitions from assets/config/abilities.ron
pub fn load_ability_definitions() -> Result<AbilityDefinitions, String> {
    let config_path = "assets/config/abilities.ron";

    let contents = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read {}: {}", config_path, e))?;

    let config: AbilitiesConfig = ron::from_str(&contents)
        .map_err(|e| format!("Failed to parse {}: {}", config_path, e))?;

    let definitions = AbilityDefinitions::new(config);

    // Validate all expected abilities are defined
    definitions.validate()
        .map_err(|missing| format!(
            "Missing ability definitions: {:?}",
            missing
        ))?;

    info!("Loaded {} ability definitions from {}", definitions.definitions.len(), config_path);

    Ok(definitions)
}

/// Bevy plugin for ability configuration loading
pub struct AbilityConfigPlugin;

impl Plugin for AbilityConfigPlugin {
    fn build(&self, app: &mut App) {
        // Load ability definitions at startup
        match load_ability_definitions() {
            Ok(definitions) => {
                app.insert_resource(definitions);
            }
            Err(e) => {
                // In development, we might want to continue with hardcoded fallback
                // For now, panic to ensure config is always valid
                panic!("Failed to load ability definitions: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ability_config_is_damage() {
        let config = AbilityConfig {
            name: "Test".to_string(),
            cast_time: 0.0,
            range: 40.0,
            mana_cost: 0.0,
            cooldown: 0.0,
            damage_base_min: 10.0,
            damage_base_max: 20.0,
            damage_coefficient: 0.5,
            damage_scales_with: ScalingStat::SpellPower,
            healing_base_min: 0.0,
            healing_base_max: 0.0,
            healing_coefficient: 0.0,
            applies_aura: None,
            projectile_speed: None,
            projectile_visuals: None,
            spell_school: SpellSchool::Frost,
            is_interrupt: false,
            lockout_duration: 0.0,
            requires_stealth: false,
            is_charge: false,
            spawn_impact_effect: false,
        };

        assert!(config.is_damage());
        assert!(!config.is_heal());
    }

    #[test]
    fn test_ability_config_is_heal() {
        let config = AbilityConfig {
            name: "Test Heal".to_string(),
            cast_time: 1.5,
            range: 40.0,
            mana_cost: 25.0,
            cooldown: 0.0,
            damage_base_min: 0.0,
            damage_base_max: 0.0,
            damage_coefficient: 0.0,
            damage_scales_with: ScalingStat::None,
            healing_base_min: 15.0,
            healing_base_max: 20.0,
            healing_coefficient: 0.75,
            applies_aura: None,
            projectile_speed: None,
            projectile_visuals: None,
            spell_school: SpellSchool::Holy,
            is_interrupt: false,
            lockout_duration: 0.0,
            requires_stealth: false,
            is_charge: false,
            spawn_impact_effect: false,
        };

        assert!(!config.is_damage());
        assert!(config.is_heal());
    }
}
