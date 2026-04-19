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

/// Get the total lockout duration reduction from LockoutDurationReduction auras on a combatant.
/// Used by Concentration Aura to reduce interrupt lockout duration.
/// Returns the percentage reduction clamped to [0.0, 1.0] (0.50 = 50% shorter lockouts).
pub fn get_lockout_duration_reduction(auras: Option<&ActiveAuras>) -> f32 {
    let total: f32 = auras.map_or(0.0, |a| {
        a.auras
            .iter()
            .filter(|aura| aura.effect_type == AuraType::LockoutDurationReduction)
            .map(|aura| aura.magnitude)
            .sum()
    });
    total.min(1.0)
}

/// Get the net attack power bonus from active auras.
/// Sums AttackPowerIncrease magnitudes and subtracts AttackPowerReduction magnitudes.
/// Can return negative (e.g., Demoralizing Shout with no Battle Shout).
pub fn get_attack_power_bonus(auras: Option<&ActiveAuras>) -> f32 {
    auras.map_or(0.0, |a| get_attack_power_bonus_from_slice(&a.auras))
}

/// Get the net attack power bonus from a slice of auras.
/// Used by class AI which stores auras as Vec<Aura> in CombatContext.
pub fn get_attack_power_bonus_from_slice(auras: &[Aura]) -> f32 {
    auras
        .iter()
        .map(|aura| match aura.effect_type {
            AuraType::AttackPowerIncrease => aura.magnitude,
            AuraType::AttackPowerReduction => -aura.magnitude,
            _ => 0.0,
        })
        .sum()
}

/// Get the total crit chance bonus from CritChanceIncrease auras.
/// Used by Molten Armor to increase crit chance dynamically.
pub fn get_crit_chance_bonus(auras: Option<&ActiveAuras>) -> f32 {
    auras.map_or(0.0, |a| get_crit_chance_bonus_from_slice(&a.auras))
}

/// Get the total crit chance bonus from a slice of auras.
/// Used by class AI which stores auras as Vec<Aura> in CombatContext.
pub fn get_crit_chance_bonus_from_slice(auras: &[Aura]) -> f32 {
    auras
        .iter()
        .filter(|aura| aura.effect_type == AuraType::CritChanceIncrease)
        .map(|aura| aura.magnitude)
        .sum()
}

/// Get the total mana regen bonus from ManaRegenIncrease auras.
/// Used by Mage Armor to increase mana regeneration dynamically.
pub fn get_mana_regen_bonus(auras: Option<&ActiveAuras>) -> f32 {
    auras.map_or(0.0, |a| {
        a.auras
            .iter()
            .filter(|aura| aura.effect_type == AuraType::ManaRegenIncrease)
            .map(|aura| aura.magnitude)
            .sum()
    })
}

/// Calculate the modified cast time accounting for CastTimeIncrease auras.
/// This should be called when starting a cast to get the actual cast duration.
pub fn calculate_cast_time(base_cast_time: f32, auras: Option<&ActiveAuras>) -> f32 {
    if base_cast_time <= 0.0 {
        return 0.0; // Instant casts aren't affected
    }
    let cast_time_increase = get_cast_time_increase(auras);
    base_cast_time * (1.0 + cast_time_increase)
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
            applied_this_frame: false,
            backlash_damage: None,
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
    // Mitigation Tracking Tests
    // =========================================================================

    /// Resistance index mapping (mirrors `resistance_school_index` in damage.rs)
    const FROST_IDX: usize = 0;
    const HOLY_IDX: usize = 1;
    const SHADOW_IDX: usize = 2;
    const ARCANE_IDX: usize = 3;
    const FIRE_IDX: usize = 4;
    const NATURE_IDX: usize = 5;

    #[test]
    fn test_combatant_mitigation_defaults_zero() {
        let combatant = Combatant::new(1, 0, match_config::CharacterClass::Warrior);
        assert_eq!(combatant.damage_mitigated_by_armor, 0.0);
        assert_eq!(combatant.damage_mitigated_by_resistance, [0.0; 6]);
    }

    #[test]
    fn test_armor_mitigation_tracked() {
        // armor=5500 → reduction = 5500 / 11000 = 0.5 → 50% mitigation
        let mut target = create_test_combatant(200.0);
        target.armor = 5500.0;

        let (actual, _) = apply_damage_with_absorb(100.0, &mut target, None, SpellSchool::Physical);

        assert_eq!(actual, 50.0, "Half should hit health");
        assert_eq!(target.damage_mitigated_by_armor, 50.0, "Half should be tracked as mitigated by armor");
        assert_eq!(target.damage_mitigated_by_resistance, [0.0; 6], "Resistance untouched for physical");
    }

    #[test]
    fn test_armor_zero_records_no_mitigation() {
        let mut target = create_test_combatant(200.0);
        target.armor = 0.0;

        let (actual, _) = apply_damage_with_absorb(100.0, &mut target, None, SpellSchool::Physical);

        assert_eq!(actual, 100.0);
        assert_eq!(target.damage_mitigated_by_armor, 0.0);
    }

    #[test]
    fn test_resistance_mitigation_tracked_per_school() {
        // resistance=60 → reduction = 60 / (60*5/3 + 300) = 60 / 400 = 0.15 → 15% mitigation
        let mut target = create_test_combatant(200.0);
        target.frost_resistance = 60.0;

        let (actual, _) = apply_damage_with_absorb(100.0, &mut target, None, SpellSchool::Frost);

        assert!((actual - 85.0).abs() < 0.001, "85 damage should hit, got {}", actual);
        assert!(
            (target.damage_mitigated_by_resistance[FROST_IDX] - 15.0).abs() < 0.001,
            "Frost slot should record 15.0, got {}",
            target.damage_mitigated_by_resistance[FROST_IDX]
        );
        assert_eq!(target.damage_mitigated_by_armor, 0.0, "Armor untouched for magical");
    }

    #[test]
    fn test_resistance_per_school_isolation() {
        // Fire resistance only — Frost damage should not write to Fire slot
        let mut target = create_test_combatant(200.0);
        target.fire_resistance = 60.0;

        let (_, _) = apply_damage_with_absorb(100.0, &mut target, None, SpellSchool::Frost);

        assert_eq!(target.damage_mitigated_by_resistance[FIRE_IDX], 0.0, "Fire slot must not record Frost damage");
        assert_eq!(target.damage_mitigated_by_resistance[FROST_IDX], 0.0, "Frost slot zero — no frost resist");
    }

    #[test]
    fn test_each_school_writes_correct_slot() {
        // Run six identical hits through six schools and verify each writes only its own slot.
        let cases = [
            (SpellSchool::Frost, FROST_IDX),
            (SpellSchool::Holy, HOLY_IDX),
            (SpellSchool::Shadow, SHADOW_IDX),
            (SpellSchool::Arcane, ARCANE_IDX),
            (SpellSchool::Fire, FIRE_IDX),
            (SpellSchool::Nature, NATURE_IDX),
        ];

        for (school, expected_idx) in cases {
            let mut target = create_test_combatant(200.0);
            // Set the matching resistance to 60 (15% reduction)
            match school {
                SpellSchool::Frost => target.frost_resistance = 60.0,
                SpellSchool::Holy => target.holy_resistance = 60.0,
                SpellSchool::Shadow => target.shadow_resistance = 60.0,
                SpellSchool::Arcane => target.arcane_resistance = 60.0,
                SpellSchool::Fire => target.fire_resistance = 60.0,
                SpellSchool::Nature => target.nature_resistance = 60.0,
                _ => unreachable!(),
            }

            let (_, _) = apply_damage_with_absorb(100.0, &mut target, None, school);

            for idx in 0..6 {
                if idx == expected_idx {
                    assert!(
                        (target.damage_mitigated_by_resistance[idx] - 15.0).abs() < 0.001,
                        "School {:?} should write 15.0 to slot {}, got {}",
                        school, idx, target.damage_mitigated_by_resistance[idx]
                    );
                } else {
                    assert_eq!(
                        target.damage_mitigated_by_resistance[idx], 0.0,
                        "School {:?} must not touch slot {}", school, idx
                    );
                }
            }
        }
    }

    #[test]
    fn test_school_none_records_no_mitigation() {
        let mut target = create_test_combatant(200.0);
        target.armor = 5500.0;
        target.frost_resistance = 60.0;

        // SpellSchool::None bypasses both armor and resistance branches
        let (actual, _) = apply_damage_with_absorb(100.0, &mut target, None, SpellSchool::None);

        assert_eq!(actual, 100.0, "None damage takes full hit");
        assert_eq!(target.damage_mitigated_by_armor, 0.0);
        assert_eq!(target.damage_mitigated_by_resistance, [0.0; 6]);
    }

    #[test]
    fn test_immunity_records_no_mitigation() {
        // Divine Shield: damage immunity returns early before any mitigation runs
        let mut target = create_test_combatant(200.0);
        target.armor = 5500.0;
        let mut auras = ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::DamageImmunity,
                duration: 8.0,
                magnitude: 0.0,
                break_on_damage_threshold: -1.0,
                accumulated_damage: 0.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                caster: None,
                ability_name: "Divine Shield".to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: None,
                applied_this_frame: false,
                backlash_damage: None,
            }],
        };

        let (actual, absorbed) = apply_damage_with_absorb(100.0, &mut target, Some(&mut auras), SpellSchool::Physical);

        assert_eq!(actual, 0.0);
        assert_eq!(absorbed, 0.0);
        assert_eq!(target.damage_mitigated_by_armor, 0.0, "Immunity is not mitigation");
    }

    #[test]
    fn test_mitigation_recorded_before_absorb_consumption() {
        // Order: armor → resistance → reduction → absorb
        // A frost shield + frost resistance: resistance fires first, then absorb
        let mut target = create_test_combatant(200.0);
        target.frost_resistance = 60.0; // 15% reduction
        let mut auras = ActiveAuras {
            auras: vec![create_absorb_aura(1000.0, "Ice Barrier")], // soak everything
        };

        let (actual, absorbed) = apply_damage_with_absorb(100.0, &mut target, Some(&mut auras), SpellSchool::Frost);

        assert_eq!(actual, 0.0, "All post-resist damage absorbed");
        assert!((absorbed - 85.0).abs() < 0.001, "85 damage absorbed, got {}", absorbed);
        assert!(
            (target.damage_mitigated_by_resistance[FROST_IDX] - 15.0).abs() < 0.001,
            "Resistance mitigation tracked even when remainder is absorbed"
        );
    }

    #[test]
    fn test_resistance_from_buff_aura_tracks_mitigation() {
        // Combatant has 0 base frost resistance but a SpellResistanceBuff aura with magnitude 60
        let mut target = create_test_combatant(200.0);
        let mut auras = ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::SpellResistanceBuff,
                duration: 300.0,
                magnitude: 60.0,
                break_on_damage_threshold: -1.0,
                accumulated_damage: 0.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                caster: None,
                ability_name: "Shadow Resistance Aura".to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                spell_school: Some(SpellSchool::Shadow),
                applied_this_frame: false,
                backlash_damage: None,
            }],
        };

        let (actual, _) = apply_damage_with_absorb(100.0, &mut target, Some(&mut auras), SpellSchool::Shadow);

        // effective resistance = 0 base + 60 aura = 60 → 15% reduction
        assert!((actual - 85.0).abs() < 0.001, "85 damage should hit, got {}", actual);
        assert!(
            (target.damage_mitigated_by_resistance[SHADOW_IDX] - 15.0).abs() < 0.001,
            "Shadow slot should record 15.0 from buff-only resistance, got {}",
            target.damage_mitigated_by_resistance[SHADOW_IDX]
        );
    }

    #[test]
    fn test_lethal_damage_records_full_armor_mitigation() {
        // Target has only 10 HP but armor mitigates 50% → mitigation is 50, not clamped to HP
        let mut target = create_test_combatant(10.0);
        target.armor = 5500.0;

        let (actual, _) = apply_damage_with_absorb(100.0, &mut target, None, SpellSchool::Physical);

        assert_eq!(actual, 10.0, "Only 10 HP was available");
        assert_eq!(target.damage_mitigated_by_armor, 50.0, "Full 50% mitigation recorded even on lethal hit");
        assert_eq!(target.current_health, 0.0);
    }

    #[test]
    fn test_mitigation_accumulates_across_hits() {
        let mut target = create_test_combatant(500.0);
        target.armor = 5500.0;

        // Three identical 100-physical hits → 50 mitigated each → 150 total
        for _ in 0..3 {
            apply_damage_with_absorb(100.0, &mut target, None, SpellSchool::Physical);
        }

        assert_eq!(target.damage_mitigated_by_armor, 150.0, "Mitigation accumulates");
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
                applied_this_frame: false,
                backlash_damage: None,
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
                applied_this_frame: false,
                backlash_damage: None,
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

    // =========================================================================
    // Dynamic Stat Bonus Helper Tests
    // =========================================================================

    fn create_aura(effect_type: AuraType, magnitude: f32) -> Aura {
        Aura {
            effect_type,
            duration: 30.0,
            magnitude,
            break_on_damage_threshold: 0.0,
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: None,
            ability_name: "Test".to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None,
            applied_this_frame: false,
            backlash_damage: None,
        }
    }

    #[test]
    fn test_get_attack_power_bonus_increase() {
        let auras = ActiveAuras {
            auras: vec![create_aura(AuraType::AttackPowerIncrease, 20.0)],
        };
        assert_eq!(get_attack_power_bonus(Some(&auras)), 20.0);
    }

    #[test]
    fn test_get_attack_power_bonus_reduction() {
        let auras = ActiveAuras {
            auras: vec![create_aura(AuraType::AttackPowerReduction, 15.0)],
        };
        assert_eq!(get_attack_power_bonus(Some(&auras)), -15.0);
    }

    #[test]
    fn test_get_attack_power_bonus_mixed() {
        let auras = ActiveAuras {
            auras: vec![
                create_aura(AuraType::AttackPowerIncrease, 20.0),
                create_aura(AuraType::AttackPowerReduction, 15.0),
            ],
        };
        assert_eq!(get_attack_power_bonus(Some(&auras)), 5.0);
    }

    #[test]
    fn test_get_attack_power_bonus_none() {
        assert_eq!(get_attack_power_bonus(None), 0.0);
    }

    #[test]
    fn test_get_attack_power_bonus_empty() {
        let auras = ActiveAuras { auras: vec![] };
        assert_eq!(get_attack_power_bonus(Some(&auras)), 0.0);
    }

    #[test]
    fn test_get_crit_chance_bonus() {
        let auras = ActiveAuras {
            auras: vec![create_aura(AuraType::CritChanceIncrease, 0.05)],
        };
        assert_eq!(get_crit_chance_bonus(Some(&auras)), 0.05);
    }

    #[test]
    fn test_get_crit_chance_bonus_none() {
        assert_eq!(get_crit_chance_bonus(None), 0.0);
    }

    #[test]
    fn test_get_crit_chance_bonus_empty() {
        let auras = ActiveAuras { auras: vec![] };
        assert_eq!(get_crit_chance_bonus(Some(&auras)), 0.0);
    }

    #[test]
    fn test_get_mana_regen_bonus() {
        let auras = ActiveAuras {
            auras: vec![create_aura(AuraType::ManaRegenIncrease, 8.0)],
        };
        assert_eq!(get_mana_regen_bonus(Some(&auras)), 8.0);
    }

    #[test]
    fn test_get_mana_regen_bonus_none() {
        assert_eq!(get_mana_regen_bonus(None), 0.0);
    }

    #[test]
    fn test_get_mana_regen_bonus_empty() {
        let auras = ActiveAuras { auras: vec![] };
        assert_eq!(get_mana_regen_bonus(Some(&auras)), 0.0);
    }
}
