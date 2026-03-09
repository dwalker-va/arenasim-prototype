use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use super::super::abilities::SpellSchool;
use super::super::ability_config::AbilityConfig;
use super::super::constants::{DR_RESET_TIMER, DR_IMMUNE_LEVEL, DR_MULTIPLIERS};

// ============================================================================
// Aura Types
// ============================================================================

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

// ============================================================================
// Aura Struct
// ============================================================================

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
    pub spell_school: Option<SpellSchool>,
}

impl Aura {
    /// Returns true if this aura can be removed by Dispel Magic.
    /// Magic-dispellable aura types (slows, roots, fear, polymorph) are always dispellable.
    /// DoTs are dispellable only if they have a magic spell school (Corruption, Immolate)
    /// but not if they're physical (Rend).
    pub fn can_be_dispelled(&self) -> bool {
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

// ============================================================================
// ActiveAuras Component
// ============================================================================

/// Component tracking active auras/debuffs on a combatant.
#[derive(Component, Default)]
pub struct ActiveAuras {
    pub auras: Vec<Aura>,
}

// ============================================================================
// AuraPending Component
// ============================================================================

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
