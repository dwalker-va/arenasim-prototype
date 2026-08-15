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
use super::components::{AuraType, DRCategory, DispelType};

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
    /// Spell-power scaling for the magnitude: effective magnitude =
    /// `magnitude + caster spell power × magnitude_coefficient` (Power Word:
    /// Shield: 25 + 0.4 × SP). Applied only at call sites using
    /// `AuraPending::from_ability_scaled` — `validate()` rejects a non-zero
    /// coefficient on any ability not in `SP_SCALED_AURA_WIRED`, so a config
    /// edit can't silently no-op.
    #[serde(default)]
    pub magnitude_coefficient: f32,
    /// Whether one caster may hold only ONE instance of this aura at a time.
    ///
    /// Landing a new one supersedes the caster's previous instance, wherever it
    /// sits. True for single-target hard crowd control (Polymorph, Fear); false
    /// for everything else, which notably keeps AoE crowd control such as
    /// Psychic Scream working — it is keyed per ABILITY, not per aura type, so
    /// a Warlock's Fear and a Priest's Psychic Scream do not supersede one
    /// another despite sharing `AuraType::Fear`.
    #[serde(default)]
    pub unique_per_caster: bool,
    /// Damage threshold that breaks the aura.
    /// - Negative (default -1.0) = doesn't break on damage
    /// - 0.0 = breaks on ANY damage (e.g., Polymorph)
    /// - Positive = breaks when accumulated damage exceeds threshold
    #[serde(default = "default_break_on_damage")]
    pub break_on_damage: f32,
    /// Tick interval for DoT effects in seconds (0.0 = no ticks)
    #[serde(default)]
    pub tick_interval: f32,
    /// Optional diminishing-returns category override for the applied aura. When
    /// omitted (the default), the DR bucket is derived from `aura_type`. Set this
    /// only when an ability needs its own bucket distinct from others sharing the
    /// same aura type — currently just Kidney Shot (`Some(KidneyShotStun)`), which
    /// must not share stun DR with Cheap Shot.
    #[serde(default)]
    pub dr_category: Option<DRCategory>,
    /// Dispel classification of the applied aura. Defaults to `Auto` (magic
    /// removability derived from the aura type). Set `Poison` for poison debuffs
    /// (Crippling Poison) so Dispel Magic can't remove them — only a cleanse can.
    #[serde(default)]
    pub dispel_type: DispelType,
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
    /// Icon asset path (e.g. "icons/abilities/spell_frost_frostbolt02.jpg")
    #[serde(default)]
    pub icon: String,
    /// Optional hand-written effect description for the tooltip. When non-empty
    /// it overrides the auto-generated description — used for effects that aren't
    /// expressible from the numeric config fields (totems, Purge, etc.).
    #[serde(default)]
    pub description: String,

    // === Casting ===
    /// Cast time in seconds (0.0 = instant)
    #[serde(default)]
    pub cast_time: f32,
    /// Maximum range in units
    pub range: f32,
    /// Minimum range in units (Hunter dead zone). None = no minimum range.
    #[serde(default)]
    pub min_range: Option<f32>,
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
    /// On-hit application chance (0.0–1.0) for proc-style effects like weapon
    /// poisons. `None` for everything else (the effect applies deterministically).
    /// Crippling Poison sets this so its slow procs probabilistically per swing.
    #[serde(default)]
    pub application_chance: Option<f32>,
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

    // === Channeling ===
    /// Duration of channel in seconds (None = not a channeled spell)
    #[serde(default)]
    pub channel_duration: Option<f32>,
    /// How often channel ticks occur (in seconds, default 1.0)
    #[serde(default = "default_channel_tick_interval")]
    pub channel_tick_interval: f32,
    /// Healing applied to caster per tick (for Drain Life style abilities)
    #[serde(default)]
    pub channel_healing_per_tick: f32,

    // === Dispel ===
    /// Whether this ability removes magic debuffs from the target
    #[serde(default)]
    pub is_dispel: bool,

    // === Mana Burn ===
    /// Mana destroyed on the target when this ability lands (Priest Mana Burn).
    /// Only affects `ResourceType::Mana` targets — Warriors reuse `current_mana`
    /// as their rage pool and must never be burned. NOT scaled by ArenaDampening:
    /// dampening throttles healing throughput; mana burn is pressure toward
    /// resolution, which is the same goal.
    #[serde(default)]
    pub mana_burn_amount: f32,

    // === Dispel Backlash ===
    /// Configuration for the dispel-backlash mechanic (currently only Unstable Affliction).
    /// When this ability's aura is removed by an enemy dispel, the dispeller takes direct
    /// Shadow damage and receives a Silence aura. See `DispelBacklashConfig`.
    #[serde(default)]
    pub dispel_backlash: Option<DispelBacklashConfig>,
}

/// Dispel-backlash configuration for abilities whose aura punishes the enemy dispeller.
///
/// Snapshot semantics: at cast time, the caster's spell power is combined with
/// `damage_base` and `damage_sp_coefficient` to produce the backlash damage, which is
/// stored on the resulting `Aura.backlash_damage`. If the aura is later dispelled by an
/// opposing-team combatant, that snapshotted damage is applied to the dispeller along
/// with a `Silence` aura of `silence_duration` seconds.
///
/// Currently only populated for Unstable Affliction.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct DispelBacklashConfig {
    /// Duration of the Silence aura applied to the dispeller (seconds). Subject to DR.
    #[serde(default)]
    pub silence_duration: f32,
    /// Flat base damage before spell-power scaling.
    #[serde(default)]
    pub damage_base: f32,
    /// Coefficient applied to the caster's spell power at cast time.
    #[serde(default)]
    pub damage_sp_coefficient: f32,
}

fn default_scaling_none() -> ScalingStat {
    ScalingStat::None
}

fn default_spell_school_none() -> SpellSchool {
    SpellSchool::None
}

fn default_channel_tick_interval() -> f32 {
    1.0
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

    /// Returns true if this is a channeled ability
    pub fn is_channel(&self) -> bool {
        self.channel_duration.is_some()
    }

    /// Resolve this ability's cast color as `(base_rgb, emissive_rgb)` in
    /// 0..1 linear-ish component form: the exact `projectile_visuals` pair when
    /// the ability defines one (the casting orb then matches the outgoing
    /// projectile), otherwise a SATURATED form of the spell-school color from
    /// [`SpellSchool::color_rgb8`]. A later per-spell override slots in here
    /// without reworking callers.
    ///
    /// The school palette is tuned for UI text on a dark background — pastel,
    /// high-luminance. Used raw as an additive emissive, those near-equal
    /// channels clip toward WHITE once the orb and motes stack (Fear's pastel
    /// Shadow purple read as white in play, Immolate's Fire likewise). So the
    /// fallback normalizes to the dominant channel and squares the lesser
    /// ones — the hue survives additive stacking — and emits at 2x (low end
    /// of the repo's 2-4x glow convention, again to delay clipping).
    pub fn cast_color(&self) -> ([f32; 3], [f32; 3]) {
        if let Some(visuals) = &self.projectile_visuals {
            return (visuals.color, visuals.emissive);
        }
        let (r, g, b) = self.spell_school.color_rgb8();
        let raw = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
        let max = raw[0].max(raw[1]).max(raw[2]).max(f32::EPSILON);
        let base = [
            (raw[0] / max) * (raw[0] / max),
            (raw[1] / max) * (raw[1] / max),
            (raw[2] / max) * (raw[2] / max),
        ];
        let emissive = [base[0] * 2.0, base[1] * 2.0, base[2] * 2.0];
        (base, emissive)
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
#[derive(Resource, Clone)]
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
            AbilityType::CheapShot,
            AbilityType::FrostNova,
            AbilityType::MindBlast,
            AbilityType::SinisterStrike,
            AbilityType::Charge,
            AbilityType::KidneyShot,
            AbilityType::PowerWordFortitude,
            AbilityType::PsychicScream,
            AbilityType::ManaBurn,
            AbilityType::Rend,
            AbilityType::MortalStrike,
            AbilityType::Pummel,
            AbilityType::BerserkerRage,
            AbilityType::Kick,
            AbilityType::CripplingPoison,
            AbilityType::Corruption,
            AbilityType::Shadowbolt,
            AbilityType::Fear,
            AbilityType::Immolate,
            AbilityType::DrainLife,
            AbilityType::CurseOfAgony,
            AbilityType::CurseOfWeakness,
            AbilityType::CurseOfTongues,
            AbilityType::UnstableAffliction,
            AbilityType::DeathCoil,
            AbilityType::ArcaneIntellect,
            AbilityType::BattleShout,
            AbilityType::IceBarrier,
            AbilityType::PowerWordShield,
            AbilityType::Polymorph,
            AbilityType::DispelMagic,
            // Paladin abilities
            AbilityType::FlashOfLight,
            AbilityType::HolyLight,
            AbilityType::HolyShock,
            AbilityType::HammerOfJustice,
            AbilityType::PaladinCleanse,
            AbilityType::DevotionAura,
            AbilityType::DivineShield,
            // Pet abilities (Felhunter)
            AbilityType::SpellLock,
            AbilityType::DevourMagic,
            // Hunter abilities
            AbilityType::AimedShot,
            AbilityType::ArcaneShot,
            AbilityType::ConcussiveShot,
            AbilityType::SerpentSting,
            AbilityType::Disengage,
            AbilityType::FreezingTrap,
            AbilityType::FrostTrap,
            // Hunter pet abilities
            AbilityType::SpiderWeb,
            AbilityType::BoarCharge,
            AbilityType::MastersCall,
            // Strategic option abilities
            AbilityType::DemoralizingShout,
            AbilityType::CommandingShout,
            AbilityType::FrostArmor,
            AbilityType::MageArmorSpell,
            AbilityType::MoltenArmor,
            AbilityType::ShadowResistanceAura,
            AbilityType::ConcentrationAura,
            // Shaman abilities
            AbilityType::LightningBolt,
            AbilityType::FrostShock,
            AbilityType::LesserHealingWave,
            AbilityType::Purge,
            AbilityType::WindShear,
            AbilityType::AirTotem,
            AbilityType::WaterTotem,
            AbilityType::EarthTotem,
            AbilityType::FireTotem,
        ];

        let missing: Vec<AbilityType> = expected_abilities
            .into_iter()
            .filter(|ability| !self.definitions.contains_key(ability))
            .collect();

        // SP-scaled aura magnitudes only take effect at call sites using
        // `AuraPending::from_ability_scaled`. Reject a non-zero coefficient on
        // any ability whose apply site isn't wired for it — otherwise the RON
        // edit silently no-ops.
        const SP_SCALED_AURA_WIRED: &[AbilityType] = &[AbilityType::PowerWordShield];
        for (ability, def) in &self.definitions {
            if let Some(aura) = &def.applies_aura {
                if aura.magnitude_coefficient != 0.0 && !SP_SCALED_AURA_WIRED.contains(ability) {
                    panic!(
                        "abilities.ron: {:?} sets applies_aura.magnitude_coefficient but its \
                         apply site uses AuraPending::from_ability (unscaled). Wire the site \
                         to from_ability_scaled and add it to SP_SCALED_AURA_WIRED.",
                        ability
                    );
                }
            }
        }

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

    /// Iterate over all ability definitions
    pub fn iter(&self) -> impl Iterator<Item = (&AbilityType, &AbilityConfig)> {
        self.definitions.iter()
    }
}

/// Load ability definitions from assets/config/abilities.ron
pub fn load_ability_definitions() -> Result<AbilityDefinitions, String> {
    let config_path = crate::paths::asset_path_str("config/abilities.ron");

    let contents = std::fs::read_to_string(&config_path)
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

    fn base_test_config() -> AbilityConfig {
        AbilityConfig {
            name: "Test".to_string(),
            icon: String::new(),
            description: String::new(),
            cast_time: 0.0,
            range: 40.0,
            min_range: None,
            mana_cost: 0.0,
            cooldown: 0.0,
            damage_base_min: 0.0,
            damage_base_max: 0.0,
            damage_coefficient: 0.0,
            damage_scales_with: ScalingStat::SpellPower,
            healing_base_min: 0.0,
            healing_base_max: 0.0,
            healing_coefficient: 0.0,
            applies_aura: None,
            application_chance: None,
            projectile_speed: None,
            projectile_visuals: None,
            spell_school: SpellSchool::None,
            is_interrupt: false,
            lockout_duration: 0.0,
            requires_stealth: false,
            is_charge: false,
            spawn_impact_effect: false,
            channel_duration: None,
            channel_tick_interval: 1.0,
            channel_healing_per_tick: 0.0,
            is_dispel: false,
            mana_burn_amount: 0.0,
            dispel_backlash: None,
        }
    }

    #[test]
    fn cast_color_prefers_projectile_visuals() {
        // Frostbolt-shaped: projectile colors defined -> orb matches the bolt
        // exactly, not the Frost school fallback.
        let mut config = base_test_config();
        config.spell_school = SpellSchool::Frost;
        config.projectile_visuals = Some(ProjectileVisuals {
            color: [0.4, 0.7, 1.0],
            emissive: [0.6, 0.9, 1.5],
        });
        let (base, emissive) = config.cast_color();
        assert_eq!(base, [0.4, 0.7, 1.0]);
        assert_eq!(emissive, [0.6, 0.9, 1.5]);
    }

    #[test]
    fn cast_color_falls_back_to_saturated_school() {
        // No projectile visuals -> saturated school color (normalize to the
        // dominant channel, square the lesser ones) with 2x emissive. Pins the
        // hue actually survives: the raw pastel palette read as white in play.
        let saturate = |rgb8: (u8, u8, u8)| -> [f32; 3] {
            let raw = [
                rgb8.0 as f32 / 255.0,
                rgb8.1 as f32 / 255.0,
                rgb8.2 as f32 / 255.0,
            ];
            let max = raw[0].max(raw[1]).max(raw[2]);
            [
                (raw[0] / max) * (raw[0] / max),
                (raw[1] / max) * (raw[1] / max),
                (raw[2] / max) * (raw[2] / max),
            ]
        };
        for school in [SpellSchool::Holy, SpellSchool::Fire, SpellSchool::Shadow] {
            let mut config = base_test_config();
            config.spell_school = school;
            let (base, emissive) = config.cast_color();
            let expected = saturate(school.color_rgb8());
            for i in 0..3 {
                assert!((base[i] - expected[i]).abs() < 1e-6, "{school:?} base");
                assert!((emissive[i] - expected[i] * 2.0).abs() < 1e-6, "{school:?} emissive");
            }
        }
        // The two spells reported as washed-out must resolve red-dominant
        // (Fire/Immolate) and blue-dominant (Shadow/Fear), not near-white.
        let fire = saturate(SpellSchool::Fire.color_rgb8());
        assert!(fire[0] > 2.0 * fire[1] && fire[0] > 2.0 * fire[2], "Fire must read red");
        let shadow = saturate(SpellSchool::Shadow.color_rgb8());
        assert!(shadow[2] > 1.5 * shadow[1], "Shadow must read purple, not white");
    }

    #[test]
    fn every_spell_school_resolves_a_color() {
        // Exhaustiveness is compiler-enforced in color_rgb8; this pins that the
        // fallback path produces a nonzero color for every school.
        for school in [
            SpellSchool::Physical,
            SpellSchool::Frost,
            SpellSchool::Holy,
            SpellSchool::Shadow,
            SpellSchool::Arcane,
            SpellSchool::Fire,
            SpellSchool::Nature,
            SpellSchool::None,
        ] {
            let mut config = base_test_config();
            config.spell_school = school;
            let (base, _) = config.cast_color();
            assert!(base.iter().any(|&c| c > 0.0), "{school:?} resolved to black");
        }
    }

    #[test]
    fn test_ability_config_is_damage() {
        let mut config = base_test_config();
        config.damage_base_min = 10.0;
        config.damage_base_max = 20.0;
        config.damage_coefficient = 0.5;
        config.damage_scales_with = ScalingStat::SpellPower;
        config.spell_school = SpellSchool::Frost;

        assert!(config.is_damage());
        assert!(!config.is_heal());
    }

    #[test]
    fn test_ability_config_is_heal() {
        let mut config = base_test_config();
        config.name = "Test Heal".to_string();
        config.cast_time = 1.5;
        config.mana_cost = 25.0;
        config.damage_scales_with = ScalingStat::None;
        config.healing_base_min = 15.0;
        config.healing_base_max = 20.0;
        config.healing_coefficient = 0.75;
        config.spell_school = SpellSchool::Holy;

        assert!(!config.is_damage());
        assert!(config.is_heal());
    }

    #[test]
    fn all_abilities_have_icons() {
        let ability_defs = load_ability_definitions().expect("abilities.ron must load");
        let mut missing: Vec<String> = Vec::new();

        for (ability_type, config) in ability_defs.iter() {
            if config.icon.is_empty() {
                missing.push(format!("{:?} ({}) has no icon", ability_type, config.name));
            }
        }

        assert!(
            missing.is_empty(),
            "Found {} ability(ies) without icons:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }
}
