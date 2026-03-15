//! Component Definitions for Play Match
//!
//! Types split into focused submodules, re-exported here for convenience.

pub mod combatant;
pub mod auras;
pub mod resources;
pub mod pets;
pub mod visual;

pub use combatant::*;
pub use auras::*;
pub use resources::*;
pub use pets::*;
pub use visual::*;

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Entity;
    use super::super::abilities::AbilityType;

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
        use super::auras::AuraPending;
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
        use super::auras::AuraPending;
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
        use super::auras::AuraPending;
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
        use super::auras::AuraPending;
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
