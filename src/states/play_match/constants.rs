//! Combat Constants
//!
//! Centralized location for magic numbers used throughout the combat system.
//! This makes it easier to tune balance and ensures consistency.

// ============================================================================
// Global Cooldown
// ============================================================================

/// Standard global cooldown duration in seconds (WoW-style 1.5s GCD)
pub const GCD: f32 = 1.5;

// ============================================================================
// Combat Ranges
// ============================================================================

/// Melee attack range in units. Combatants must be within this distance to auto-attack.
/// Similar to WoW's melee range of ~5 yards.
pub const MELEE_RANGE: f32 = 2.5;

/// Ranged wand attack range for caster classes.
/// Similar to WoW's wand range of ~30 yards.
pub const WAND_RANGE: f32 = 30.0;

/// Minimum range required to use Charge ability.
/// Can't charge if already in melee range.
pub const CHARGE_MIN_RANGE: f32 = 8.0;

/// Safe distance for kiting behavior.
/// Mages will prioritize movement over casting when closer than this to enemies.
pub const SAFE_KITING_DISTANCE: f32 = 8.0;

/// Holy Shock damage range (20 yards).
/// Heal range uses the ability config (40 yards), but damage has a shorter range.
pub const HOLY_SHOCK_DAMAGE_RANGE: f32 = 20.0;

// ============================================================================
// Health Thresholds
// ============================================================================

/// HP threshold for defensive abilities (Ice Barrier recast, etc.)
/// Below this %, defensive cooldowns become higher priority.
pub const DEFENSIVE_HP_THRESHOLD: f32 = 0.8;

/// HP threshold for emergency healing
/// Below this %, healers prioritize Flash Heal over other abilities.
pub const EMERGENCY_HEAL_THRESHOLD: f32 = 0.7;

/// HP threshold for shielding allies
/// Below this %, priests will apply Power Word: Shield.
pub const SHIELD_HP_THRESHOLD: f32 = 0.9;

/// HP threshold considered "low" for general AI decisions
pub const LOW_HP_THRESHOLD: f32 = 0.5;

/// HP threshold for critical/emergency situations (Paladin Holy Shock heal)
pub const CRITICAL_HP_THRESHOLD: f32 = 0.4;

/// HP threshold above which allies are considered "healthy" (dispel maintenance)
pub const HEALTHY_HP_THRESHOLD: f32 = 0.7;

/// HP threshold for safe long casts (Holy Light)
pub const SAFE_HEAL_MAX_THRESHOLD: f32 = 0.85;

// ============================================================================
// Damage Over Time
// ============================================================================

/// Standard tick interval for DoT effects in seconds.
/// Most DoTs tick every 3 seconds (Rend, Corruption, SW:Pain).
pub const DOT_TICK_INTERVAL: f32 = 3.0;

// ============================================================================
// Arena
// ============================================================================

/// Arena half-size on X axis (inside the visual walls).
/// The arena is 76x46 with wall centers at ±38/±23 and wall thickness 1.0.
/// We subtract 1.5 to account for wall thickness (0.5) + combatant buffer (1.0).
pub const ARENA_HALF_X: f32 = 36.5;

/// Arena half-size on Z axis (inside the visual walls).
pub const ARENA_HALF_Z: f32 = 21.5;

/// Maximum |x| + |z| permitted at octagonal arena corners (diagonal wall boundary with wall+buffer offset).
pub const ARENA_CORNER_SUM: f32 = 48.88;

// ============================================================================
// Visual/UI
// ============================================================================

/// Floating combat text base height above combatants (in world space Y units).
/// Should be high enough to avoid overlapping with status effect labels.
pub const FCT_HEIGHT: f32 = 4.0;

/// Speech bubble display duration in seconds.
pub const SPEECH_BUBBLE_DURATION: f32 = 2.0;

// ============================================================================
// Critical Strike
// ============================================================================

/// Critical strike damage multiplier (2x in WoW Classic for melee; we use 2x for all damage)
pub const CRIT_DAMAGE_MULTIPLIER: f32 = 2.0;

/// Critical strike healing multiplier (1.5x in WoW Classic)
pub const CRIT_HEALING_MULTIPLIER: f32 = 1.5;

// ============================================================================
// Divine Shield
// ============================================================================

/// Outgoing damage penalty while Divine Shield is active (50% reduction)
pub const DIVINE_SHIELD_DAMAGE_PENALTY: f32 = 0.5;

/// HP threshold for AI to activate Divine Shield (30% HP)
pub const DIVINE_SHIELD_HP_THRESHOLD: f32 = 0.3;

// ============================================================================
// Timing
// ============================================================================

/// Pet slots start at this offset. Pet of slot 0 = 10, slot 1 = 11, etc.
pub const PET_SLOT_BASE: u8 = 10;

/// Pre-match countdown duration before gates open (in seconds).
pub const PREMATCH_COUNTDOWN: f32 = 10.0;

/// Victory celebration duration before transitioning to results (in seconds).
pub const VICTORY_CELEBRATION_DURATION: f32 = 5.0;

// ============================================================================
// Hunter
// ============================================================================

/// Hunter dead zone range — ranged abilities cannot be used within this distance.
pub const HUNTER_DEAD_ZONE: f32 = 8.0;

/// Auto Shot range for Hunter ranged auto-attacks.
pub const AUTO_SHOT_RANGE: f32 = 35.0;

/// Range at which Hunter proactively kites to maintain distance.
pub const HUNTER_KITE_RANGE: f32 = 30.0;

/// Delay in seconds before a placed trap becomes armed and can be triggered.
pub const TRAP_ARM_DELAY: f32 = 1.5;

/// Radius around an armed trap that triggers it when an enemy enters.
pub const TRAP_TRIGGER_RADIUS: f32 = 5.0;

/// Radius of the Frost Trap slow zone after triggering.
pub const FROST_TRAP_ZONE_RADIUS: f32 = 8.0;

/// Duration of the Frost Trap slow zone in seconds.
pub const FROST_TRAP_ZONE_DURATION: f32 = 10.0;

/// Minimum distance from Hunter to target for trap to be "launched" (arc projectile).
/// Within this range, traps drop instantly at feet.
pub const TRAP_LAUNCH_MIN_RANGE: f32 = 10.0;

/// Horizontal travel speed of a launched trap projectile (units per second).
pub const TRAP_LAUNCH_SPEED: f32 = 20.0;

/// Peak Y offset of the parabolic arc at midpoint of trap launch travel.
pub const TRAP_LAUNCH_ARC_HEIGHT: f32 = 6.0;

/// Distance in units that Disengage launches the Hunter backward.
pub const DISENGAGE_DISTANCE: f32 = 15.0;

/// Speed of the Disengage backward leap (units per second).
pub const DISENGAGE_SPEED: f32 = 30.0;

// ============================================================================
// Diminishing Returns
// ============================================================================

/// Time in seconds before DR resets after last CC application (WoW Classic: 15s).
pub const DR_RESET_TIMER: f32 = 15.0;

/// DR level at which target becomes immune to that CC category.
pub const DR_IMMUNE_LEVEL: u8 = 3;

/// Duration multipliers indexed by DR level: 100% → 50% → 25% → Immune.
pub const DR_MULTIPLIERS: [f32; 4] = [1.0, 0.5, 0.25, 0.0];

// ============================================================================
// Item Budget Validation
// ============================================================================

use super::equipment::ItemSlot;

/// Budget points granted per item level. Effective budget = item_level * BUDGET_PER_ILVL * slot_multiplier.
/// Calibrated against the current item pool (ilvl 54-60 range).
pub const BUDGET_PER_ILVL: f32 = 0.75;

/// Tolerance for over-budget items. Items may exceed their computed budget by this fraction
/// before being flagged (0.10 = 10% over-budget allowed).
pub const BUDGET_TOLERANCE: f32 = 0.10;

/// Cost weight per point of max_health in the item budget.
pub const WEIGHT_MAX_HEALTH: f32 = 1.0;

/// Cost weight per point of max_mana in the item budget.
pub const WEIGHT_MAX_MANA: f32 = 1.0;

/// Cost weight per point of mana_regen (MP5) in the item budget.
pub const WEIGHT_MANA_REGEN: f32 = 5.0;

/// Cost weight per point of attack_power in the item budget.
pub const WEIGHT_ATTACK_POWER: f32 = 1.5;

/// Cost weight per point of spell_power in the item budget.
pub const WEIGHT_SPELL_POWER: f32 = 1.5;

/// Cost weight per fraction-point of crit_chance in the item budget.
/// Since crit is stored as a fraction (0.01 = 1%), this weight is large
/// so that 0.02 crit (2%) costs 6.0 budget points.
pub const WEIGHT_CRIT_CHANCE: f32 = 300.0;

/// Cost weight per fraction-point of movement_speed in the item budget.
/// Since movement_speed is stored as a fraction (0.1 = 10% speed),
/// this weight is large so that 0.1 speed costs 3.0 budget points.
pub const WEIGHT_MOVEMENT_SPEED: f32 = 30.0;

/// Cost weight per point of elemental resistance in the item budget.
/// Applied equally to all six resistance types (fire, frost, shadow, arcane, nature, holy).
/// Lower than core stats since resist gear trades stat efficiency for specialized protection.
pub const WEIGHT_RESISTANCE: f32 = 0.4;

/// Returns the WoW Classic-accurate slot budget multiplier for the given item slot.
/// Higher multiplier = more stat budget available. Head/Chest get the full budget,
/// while accessories like rings and trinkets get roughly half.
pub fn slot_budget_multiplier(slot: ItemSlot) -> f32 {
    match slot {
        ItemSlot::Head => 1.0,
        ItemSlot::Chest => 1.0,
        ItemSlot::Legs => 0.875,
        ItemSlot::Shoulders => 0.75,
        ItemSlot::Hands => 0.75,
        ItemSlot::Feet => 0.75,
        ItemSlot::Waist => 0.625,
        ItemSlot::Wrists => 0.5,
        ItemSlot::Neck => 0.5625,
        ItemSlot::Back => 0.5625,
        ItemSlot::Ring1 => 0.5625,
        ItemSlot::Ring2 => 0.5625,
        ItemSlot::Trinket1 => 0.5625,
        ItemSlot::Trinket2 => 0.5625,
        ItemSlot::MainHand => 0.5625,
        ItemSlot::OffHand => 0.5625,
        ItemSlot::Ranged => 0.5625,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_constants_are_positive() {
        assert!(MELEE_RANGE > 0.0);
        assert!(WAND_RANGE > 0.0);
        assert!(CHARGE_MIN_RANGE > 0.0);
        assert!(SAFE_KITING_DISTANCE > 0.0);
    }

    #[test]
    fn test_hp_thresholds_are_valid() {
        assert!(DEFENSIVE_HP_THRESHOLD > 0.0 && DEFENSIVE_HP_THRESHOLD <= 1.0);
        assert!(EMERGENCY_HEAL_THRESHOLD > 0.0 && EMERGENCY_HEAL_THRESHOLD <= 1.0);
        assert!(SHIELD_HP_THRESHOLD > 0.0 && SHIELD_HP_THRESHOLD <= 1.0);
        assert!(LOW_HP_THRESHOLD > 0.0 && LOW_HP_THRESHOLD <= 1.0);
    }

    #[test]
    fn test_gcd_is_standard_wow_value() {
        assert_eq!(GCD, 1.5);
    }

    #[test]
    fn test_slot_budget_multiplier_all_values() {
        assert_eq!(slot_budget_multiplier(ItemSlot::Head), 1.0);
        assert_eq!(slot_budget_multiplier(ItemSlot::Chest), 1.0);
        assert_eq!(slot_budget_multiplier(ItemSlot::Legs), 0.875);
        assert_eq!(slot_budget_multiplier(ItemSlot::Shoulders), 0.75);
        assert_eq!(slot_budget_multiplier(ItemSlot::Hands), 0.75);
        assert_eq!(slot_budget_multiplier(ItemSlot::Feet), 0.75);
        assert_eq!(slot_budget_multiplier(ItemSlot::Waist), 0.625);
        assert_eq!(slot_budget_multiplier(ItemSlot::Wrists), 0.5);
        assert_eq!(slot_budget_multiplier(ItemSlot::Neck), 0.5625);
        assert_eq!(slot_budget_multiplier(ItemSlot::Back), 0.5625);
        assert_eq!(slot_budget_multiplier(ItemSlot::Ring1), 0.5625);
        assert_eq!(slot_budget_multiplier(ItemSlot::Ring2), 0.5625);
        assert_eq!(slot_budget_multiplier(ItemSlot::Trinket1), 0.5625);
        assert_eq!(slot_budget_multiplier(ItemSlot::Trinket2), 0.5625);
        assert_eq!(slot_budget_multiplier(ItemSlot::MainHand), 0.5625);
        assert_eq!(slot_budget_multiplier(ItemSlot::OffHand), 0.5625);
        assert_eq!(slot_budget_multiplier(ItemSlot::Ranged), 0.5625);
    }

    #[test]
    fn test_stat_weights_are_positive() {
        assert!(WEIGHT_MAX_HEALTH > 0.0);
        assert!(WEIGHT_MAX_MANA > 0.0);
        assert!(WEIGHT_MANA_REGEN > 0.0);
        assert!(WEIGHT_ATTACK_POWER > 0.0);
        assert!(WEIGHT_SPELL_POWER > 0.0);
        assert!(WEIGHT_CRIT_CHANCE > 0.0);
        assert!(WEIGHT_MOVEMENT_SPEED > 0.0);
        assert!(WEIGHT_RESISTANCE > 0.0);
    }
}
