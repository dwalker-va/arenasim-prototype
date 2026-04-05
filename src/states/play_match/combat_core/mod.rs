//! Combat Core Systems
//!
//! Handles core combat mechanics:
//! - Movement (move_to_target, kiting logic)
//! - Auto-attacks (melee and ranged wand attacks)
//! - Resource regeneration (Energy, Rage)
//! - Casting (cast time processing, completion)
//! - Interrupt processing (applying lockouts)
//! - Stealth visuals

mod damage;
mod movement;
mod auto_attack;
mod casting;
mod death;

pub use damage::*;
pub use movement::*;
pub use auto_attack::*;
pub use casting::*;
pub use death::*;

use bevy::prelude::*;
use super::components::*;
use super::{ARENA_HALF_X, ARENA_HALF_Z, ARENA_CORNER_SUM};

// Re-export combatant_id for backward compatibility (used by other modules)
pub use super::utils::combatant_id;

/// Returns true if the XZ position is inside the octagonal arena bounds.
pub fn is_in_arena_bounds(pos: Vec3) -> bool {
    if pos.x < -ARENA_HALF_X || pos.x > ARENA_HALF_X { return false; }
    if pos.z < -ARENA_HALF_Z || pos.z > ARENA_HALF_Z { return false; }
    // Diagonal corner check: each 45° wall constrains |x|+|z|
    pos.x.abs() + pos.z.abs() <= ARENA_CORNER_SUM
}

/// Clamp a position to stay inside the octagonal arena.
pub fn clamp_to_arena(mut pos: Vec3) -> Vec3 {
    // Rectangular edges
    pos.x = pos.x.clamp(-ARENA_HALF_X, ARENA_HALF_X);
    pos.z = pos.z.clamp(-ARENA_HALF_Z, ARENA_HALF_Z);
    // Diagonal corners: project inward along the 45° normal
    let corner_excess = pos.x.abs() + pos.z.abs() - ARENA_CORNER_SUM;
    if corner_excess > 0.0 {
        let half = corner_excess / 2.0;
        pos.x -= half * pos.x.signum();
        pos.z -= half * pos.z.signum();
    }
    pos
}

/// Get the total cast time increase from CastTimeIncrease auras on a combatant.
/// Used by Curse of Tongues to slow casting.
/// Returns the percentage increase (0.5 = 50% slower, so multiply cast time by 1.5).
pub fn get_cast_time_increase(auras: Option<&ActiveAuras>) -> f32 {
    auras.map_or(0.0, |a| {
        a.auras
            .iter()
            .filter(|aura| aura.effect_type == AuraType::CastTimeIncrease)
            .map(|aura| aura.magnitude)
            .sum()
    })
}

/// Get the total cast time reduction from CastTimeReduction auras on a combatant.
/// Used by Concentration Aura to speed up casting.
/// Returns the percentage reduction (0.15 = 15% faster, so multiply cast time by 0.85).
pub fn get_cast_time_reduction(auras: Option<&ActiveAuras>) -> f32 {
    auras.map_or(0.0, |a| {
        a.auras
            .iter()
            .filter(|aura| aura.effect_type == AuraType::CastTimeReduction)
            .map(|aura| aura.magnitude)
            .sum()
    })
}

/// Calculate the modified cast time accounting for CastTimeIncrease and CastTimeReduction auras.
/// This should be called when starting a cast to get the actual cast duration.
pub fn calculate_cast_time(base_cast_time: f32, auras: Option<&ActiveAuras>) -> f32 {
    if base_cast_time <= 0.0 {
        return 0.0; // Instant casts aren't affected
    }
    let cast_time_increase = get_cast_time_increase(auras);
    let cast_time_reduction = get_cast_time_reduction(auras);
    // Apply increase first, then reduction (multiplicative)
    (base_cast_time * (1.0 + cast_time_increase) * (1.0 - cast_time_reduction)).max(0.0)
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::match_config;
    use super::super::abilities::SpellSchool;

    /// Helper to create a test combatant
    fn create_test_combatant(health: f32) -> Combatant {
        let mut combatant = Combatant::new(1, 0, match_config::CharacterClass::Warrior);
        combatant.max_health = health;
        combatant.current_health = health;
        combatant.damage_taken = 0.0;
        combatant
    }

    /// Helper to create an absorb aura
    fn create_absorb_aura(amount: f32, ability_name: &str) -> Aura {
        Aura {
            effect_type: AuraType::Absorb,
            duration: 30.0,
            magnitude: amount,
            break_on_damage_threshold: 0.0,
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: None,
            ability_name: ability_name.to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None,
        }
    }

    // =========================================================================
    // apply_damage_with_absorb Tests
    // =========================================================================

    #[test]
    fn test_damage_with_no_shields() {
        let mut target = create_test_combatant(100.0);

        let (actual_damage, absorbed) = apply_damage_with_absorb(30.0, &mut target, None, SpellSchool::None);

        assert_eq!(actual_damage, 30.0, "All damage should hit health");
        assert_eq!(absorbed, 0.0, "No damage should be absorbed");
        assert_eq!(target.current_health, 70.0, "Health should decrease by damage");
        assert_eq!(target.damage_taken, 30.0, "Damage taken should be tracked");
    }

    #[test]
    fn test_damage_fully_absorbed_by_shield() {
        let mut target = create_test_combatant(100.0);
        let mut auras = ActiveAuras {
            auras: vec![create_absorb_aura(50.0, "Power Word: Shield")],
        };

        let (actual_damage, absorbed) = apply_damage_with_absorb(30.0, &mut target, Some(&mut auras), SpellSchool::None);

        assert_eq!(actual_damage, 0.0, "No damage should hit health");
        assert_eq!(absorbed, 30.0, "All damage should be absorbed");
        assert_eq!(target.current_health, 100.0, "Health should remain full");
        assert_eq!(auras.auras[0].magnitude, 20.0, "Shield should have 20 remaining");
    }

    #[test]
    fn test_damage_partially_absorbed() {
        let mut target = create_test_combatant(100.0);
        let mut auras = ActiveAuras {
            auras: vec![create_absorb_aura(20.0, "Power Word: Shield")],
        };

        let (actual_damage, absorbed) = apply_damage_with_absorb(50.0, &mut target, Some(&mut auras), SpellSchool::None);

        assert_eq!(absorbed, 20.0, "Shield should absorb its full amount");
        assert_eq!(actual_damage, 30.0, "Remaining damage should hit health");
        assert_eq!(target.current_health, 70.0, "Health should decrease by remaining damage");
        assert!(auras.auras.is_empty(), "Depleted shield should be removed");
    }

    #[test]
    fn test_multiple_shields_stack() {
        let mut target = create_test_combatant(100.0);
        let mut auras = ActiveAuras {
            auras: vec![
                create_absorb_aura(30.0, "Power Word: Shield"),
                create_absorb_aura(40.0, "Ice Barrier"),
            ],
        };

        let (actual_damage, absorbed) = apply_damage_with_absorb(50.0, &mut target, Some(&mut auras), SpellSchool::None);

        assert_eq!(absorbed, 50.0, "All damage should be absorbed by combined shields");
        assert_eq!(actual_damage, 0.0, "No damage should hit health");
        assert_eq!(target.current_health, 100.0, "Health should remain full");

        // First shield should be consumed, second should have remaining
        assert_eq!(auras.auras.len(), 1, "One shield should remain");
        assert_eq!(auras.auras[0].magnitude, 20.0, "Ice Barrier should have 20 remaining");
    }

    #[test]
    fn test_damage_exceeds_health() {
        let mut target = create_test_combatant(50.0);

        let (actual_damage, absorbed) = apply_damage_with_absorb(100.0, &mut target, None, SpellSchool::None);

        assert_eq!(actual_damage, 50.0, "Actual damage should be limited by remaining health");
        assert_eq!(absorbed, 0.0, "No damage absorbed");
        assert_eq!(target.current_health, 0.0, "Target should be dead");
    }

    #[test]
    fn test_zero_damage() {
        let mut target = create_test_combatant(100.0);

        let (actual_damage, absorbed) = apply_damage_with_absorb(0.0, &mut target, None, SpellSchool::None);

        assert_eq!(actual_damage, 0.0, "No damage dealt");
        assert_eq!(absorbed, 0.0, "No damage absorbed");
        assert_eq!(target.current_health, 100.0, "Health unchanged");
    }

    #[test]
    fn test_depleted_shield_removed() {
        let mut target = create_test_combatant(100.0);
        let mut auras = ActiveAuras {
            auras: vec![create_absorb_aura(25.0, "Power Word: Shield")],
        };

        let (actual_damage, absorbed) = apply_damage_with_absorb(25.0, &mut target, Some(&mut auras), SpellSchool::None);

        assert_eq!(absorbed, 25.0);
        assert_eq!(actual_damage, 0.0);
        assert!(auras.auras.is_empty(), "Exactly-depleted shield should be removed");
    }

    // =========================================================================
    // has_absorb_shield Tests
    // =========================================================================

    #[test]
    fn test_has_absorb_shield_with_no_auras() {
        assert!(!has_absorb_shield(None));
    }

    #[test]
    fn test_has_absorb_shield_with_empty_auras() {
        let auras = ActiveAuras { auras: vec![] };
        assert!(!has_absorb_shield(Some(&auras)));
    }

    #[test]
    fn test_has_absorb_shield_with_absorb() {
        let auras = ActiveAuras {
            auras: vec![create_absorb_aura(50.0, "Power Word: Shield")],
        };
        assert!(has_absorb_shield(Some(&auras)));
    }

    #[test]
    fn test_has_absorb_shield_with_other_auras() {
        let auras = ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::MovementSpeedSlow,
                duration: 5.0,
                magnitude: 0.7,
                break_on_damage_threshold: 0.0,
                accumulated_damage: 0.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                caster: None,
                ability_name: "Frostbolt".to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: None,
            }],
        };
        assert!(!has_absorb_shield(Some(&auras)));
    }

    // =========================================================================
    // has_weakened_soul Tests
    // =========================================================================

    #[test]
    fn test_has_weakened_soul_with_no_auras() {
        assert!(!has_weakened_soul(None));
    }

    #[test]
    fn test_has_weakened_soul_with_weakened_soul() {
        let auras = ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::WeakenedSoul,
                duration: 15.0,
                magnitude: 0.0,
                break_on_damage_threshold: 0.0,
                accumulated_damage: 0.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                caster: None,
                ability_name: "Weakened Soul".to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: None,
            }],
        };
        assert!(has_weakened_soul(Some(&auras)));
    }

    // =========================================================================
    // combatant_id Tests
    // =========================================================================

    #[test]
    fn test_combatant_id_format() {
        let id = combatant_id(1, match_config::CharacterClass::Warrior);
        assert_eq!(id, "Team 1 Warrior");
    }

    #[test]
    fn test_combatant_id_team2() {
        let id = combatant_id(2, match_config::CharacterClass::Mage);
        assert_eq!(id, "Team 2 Mage");
    }

    // =========================================================================
    // ease_out_quad Tests
    // =========================================================================

    #[test]
    fn test_ease_out_quad_boundaries() {
        assert_eq!(death::ease_out_quad_for_test(0.0), 0.0, "Should return 0 at t=0");
        assert_eq!(death::ease_out_quad_for_test(1.0), 1.0, "Should return 1 at t=1");
    }

    #[test]
    fn test_ease_out_quad_midpoint() {
        let mid = death::ease_out_quad_for_test(0.5);
        assert!(mid > 0.5, "Ease-out should be > 0.5 at t=0.5, got {}", mid);
        assert!(mid < 1.0, "Ease-out should be < 1.0 at t=0.5, got {}", mid);
    }

    // =========================================================================
    // Arena Boundary Tests
    // =========================================================================

    #[test]
    fn test_is_in_arena_bounds_center() {
        assert!(is_in_arena_bounds(Vec3::new(0.0, 0.0, 0.0)));
    }

    #[test]
    fn test_is_in_arena_bounds_outside_x() {
        assert!(!is_in_arena_bounds(Vec3::new(40.0, 0.0, 0.0)));
    }

    #[test]
    fn test_is_in_arena_bounds_outside_z() {
        assert!(!is_in_arena_bounds(Vec3::new(0.0, 0.0, 25.0)));
    }

    #[test]
    fn test_is_in_arena_bounds_outside_diagonal_corner() {
        // Inside rectangle but outside diagonal: |30| + |20| = 50 > 48.88
        assert!(!is_in_arena_bounds(Vec3::new(30.0, 0.0, 20.0)));
    }

    #[test]
    fn test_is_in_arena_bounds_inside_diagonal_corner() {
        // |25| + |15| = 40 < 48.88
        assert!(is_in_arena_bounds(Vec3::new(25.0, 0.0, 15.0)));
    }

    #[test]
    fn test_clamp_to_arena_inside_unchanged() {
        let pos = Vec3::new(10.0, 5.0, 8.0);
        let clamped = clamp_to_arena(pos);
        assert_eq!(clamped, pos);
    }

    #[test]
    fn test_clamp_to_arena_outside_x() {
        let clamped = clamp_to_arena(Vec3::new(50.0, 1.0, 0.0));
        assert_eq!(clamped.x, ARENA_HALF_X);
        assert_eq!(clamped.z, 0.0);
    }

    #[test]
    fn test_clamp_to_arena_diagonal_corner() {
        // (35, 20) is inside rectangle but |35|+|20|=55 > 48.88
        let clamped = clamp_to_arena(Vec3::new(35.0, 1.0, 20.0));
        let sum = clamped.x.abs() + clamped.z.abs();
        assert!((sum - ARENA_CORNER_SUM).abs() < 0.01, "Corner sum should equal ARENA_CORNER_SUM, got {}", sum);
        assert!(clamped.x > 0.0, "Should stay in same quadrant");
        assert!(clamped.z > 0.0, "Should stay in same quadrant");
    }

    #[test]
    fn test_clamp_to_arena_preserves_y() {
        let clamped = clamp_to_arena(Vec3::new(50.0, 3.5, 30.0));
        assert_eq!(clamped.y, 3.5, "Y should be unchanged");
    }

    #[test]
    fn test_clamp_to_arena_idempotent() {
        let pos = Vec3::new(35.0, 1.0, 20.0);
        let once = clamp_to_arena(pos);
        let twice = clamp_to_arena(once);
        assert_eq!(once, twice, "Clamping twice should give the same result");
    }
}
