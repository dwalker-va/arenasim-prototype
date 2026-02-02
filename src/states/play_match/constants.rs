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

/// Ranged wand attack range for caster classes (Mage, Priest).
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

// ============================================================================
// Visual/UI
// ============================================================================

/// Floating combat text base height above combatants (in world space Y units).
/// Should be high enough to avoid overlapping with status effect labels.
pub const FCT_HEIGHT: f32 = 4.0;

/// Speech bubble display duration in seconds.
pub const SPEECH_BUBBLE_DURATION: f32 = 2.0;

// ============================================================================
// Timing
// ============================================================================

/// Pre-match countdown duration before gates open (in seconds).
pub const PREMATCH_COUNTDOWN: f32 = 10.0;

/// Victory celebration duration before transitioning to results (in seconds).
pub const VICTORY_CELEBRATION_DURATION: f32 = 5.0;

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
}
