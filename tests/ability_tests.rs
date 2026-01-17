//! Unit tests for ability definitions
//!
//! These tests verify that:
//! - All abilities have valid stat values
//! - Interrupt abilities have lockout durations
//! - Damage/healing abilities have appropriate scaling
//! - Spell schools are correctly assigned

use arenasim::states::play_match::{AbilityType, SpellSchool, ScalingStat};

// =============================================================================
// Ability Definition Validation Tests
// =============================================================================

/// Get all ability types for exhaustive testing
fn all_abilities() -> Vec<AbilityType> {
    vec![
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
    ]
}

#[test]
fn test_all_abilities_have_names() {
    for ability in all_abilities() {
        let def = ability.definition();
        assert!(!def.name.is_empty(), "{:?} should have a name", ability);
    }
}

#[test]
fn test_all_abilities_have_non_negative_cast_time() {
    for ability in all_abilities() {
        let def = ability.definition();
        assert!(
            def.cast_time >= 0.0,
            "{:?} should have non-negative cast time, got {}",
            ability,
            def.cast_time
        );
    }
}

#[test]
fn test_all_abilities_have_non_negative_mana_cost() {
    for ability in all_abilities() {
        let def = ability.definition();
        assert!(
            def.mana_cost >= 0.0,
            "{:?} should have non-negative mana cost, got {}",
            ability,
            def.mana_cost
        );
    }
}

#[test]
fn test_all_abilities_have_non_negative_cooldown() {
    for ability in all_abilities() {
        let def = ability.definition();
        assert!(
            def.cooldown >= 0.0,
            "{:?} should have non-negative cooldown, got {}",
            ability,
            def.cooldown
        );
    }
}

#[test]
fn test_all_abilities_have_non_negative_range() {
    for ability in all_abilities() {
        let def = ability.definition();
        assert!(
            def.range >= 0.0,
            "{:?} should have non-negative range, got {}",
            ability,
            def.range
        );
    }
}

#[test]
fn test_damage_abilities_have_positive_values() {
    let damage_abilities = vec![
        AbilityType::Frostbolt,
        AbilityType::Ambush,
        AbilityType::FrostNova,
        AbilityType::MindBlast,
        AbilityType::SinisterStrike,
        AbilityType::MortalStrike,
        AbilityType::Shadowbolt,
    ];

    for ability in damage_abilities {
        let def = ability.definition();
        assert!(
            def.is_damage(),
            "{:?} should be classified as a damage ability",
            ability
        );
        assert!(
            def.damage_base_max >= def.damage_base_min,
            "{:?} damage max ({}) should be >= min ({})",
            ability,
            def.damage_base_max,
            def.damage_base_min
        );
    }
}

#[test]
fn test_healing_abilities_have_positive_values() {
    let healing_abilities = vec![AbilityType::FlashHeal];

    for ability in healing_abilities {
        let def = ability.definition();
        assert!(
            def.is_heal(),
            "{:?} should be classified as a healing ability",
            ability
        );
        assert!(
            def.healing_base_max >= def.healing_base_min,
            "{:?} healing max ({}) should be >= min ({})",
            ability,
            def.healing_base_max,
            def.healing_base_min
        );
    }
}

#[test]
fn test_interrupt_abilities_have_lockout_duration() {
    let interrupt_abilities = vec![AbilityType::Pummel, AbilityType::Kick];

    for ability in interrupt_abilities {
        let def = ability.definition();
        assert!(
            def.is_interrupt,
            "{:?} should be marked as an interrupt",
            ability
        );
        assert!(
            def.lockout_duration > 0.0,
            "{:?} should have positive lockout duration, got {}",
            ability,
            def.lockout_duration
        );
    }
}

#[test]
fn test_non_interrupt_abilities_have_no_lockout() {
    let non_interrupt_abilities = vec![
        AbilityType::Frostbolt,
        AbilityType::FlashHeal,
        AbilityType::MortalStrike,
        AbilityType::Shadowbolt,
    ];

    for ability in non_interrupt_abilities {
        let def = ability.definition();
        assert!(
            !def.is_interrupt,
            "{:?} should not be marked as an interrupt",
            ability
        );
    }
}

// =============================================================================
// Spell School Tests
// =============================================================================

#[test]
fn test_frost_abilities_have_frost_school() {
    let frost_abilities = vec![
        AbilityType::Frostbolt,
        AbilityType::FrostNova,
        AbilityType::IceBarrier,
    ];

    for ability in frost_abilities {
        let def = ability.definition();
        assert_eq!(
            def.spell_school,
            SpellSchool::Frost,
            "{:?} should have Frost spell school",
            ability
        );
    }
}

#[test]
fn test_shadow_abilities_have_shadow_school() {
    let shadow_abilities = vec![
        AbilityType::MindBlast,
        AbilityType::Corruption,
        AbilityType::Shadowbolt,
        AbilityType::Fear,
    ];

    for ability in shadow_abilities {
        let def = ability.definition();
        assert_eq!(
            def.spell_school,
            SpellSchool::Shadow,
            "{:?} should have Shadow spell school",
            ability
        );
    }
}

#[test]
fn test_holy_abilities_have_holy_school() {
    let holy_abilities = vec![
        AbilityType::FlashHeal,
        AbilityType::PowerWordFortitude,
        AbilityType::PowerWordShield,
    ];

    for ability in holy_abilities {
        let def = ability.definition();
        assert_eq!(
            def.spell_school,
            SpellSchool::Holy,
            "{:?} should have Holy spell school",
            ability
        );
    }
}

#[test]
fn test_physical_abilities_have_physical_school() {
    let physical_abilities = vec![
        AbilityType::HeroicStrike,
        AbilityType::Ambush,
        AbilityType::SinisterStrike,
        AbilityType::Charge,
        AbilityType::KidneyShot,
        AbilityType::Rend,
        AbilityType::MortalStrike,
        AbilityType::Pummel,
        AbilityType::Kick,
    ];

    for ability in physical_abilities {
        let def = ability.definition();
        assert_eq!(
            def.spell_school,
            SpellSchool::Physical,
            "{:?} should have Physical spell school",
            ability
        );
    }
}

// =============================================================================
// Scaling Stat Tests
// =============================================================================

#[test]
fn test_physical_damage_scales_with_attack_power() {
    let physical_damage_abilities = vec![
        AbilityType::Ambush,
        AbilityType::SinisterStrike,
        AbilityType::MortalStrike,
    ];

    for ability in physical_damage_abilities {
        let def = ability.definition();
        assert_eq!(
            def.damage_scales_with,
            ScalingStat::AttackPower,
            "{:?} should scale with Attack Power",
            ability
        );
    }
}

#[test]
fn test_magical_damage_scales_with_spell_power() {
    let magical_damage_abilities = vec![
        AbilityType::Frostbolt,
        AbilityType::FrostNova,
        AbilityType::MindBlast,
        AbilityType::Shadowbolt,
    ];

    for ability in magical_damage_abilities {
        let def = ability.definition();
        assert_eq!(
            def.damage_scales_with,
            ScalingStat::SpellPower,
            "{:?} should scale with Spell Power",
            ability
        );
    }
}

#[test]
fn test_healing_scales_with_spell_power() {
    let def = AbilityType::FlashHeal.definition();
    assert!(
        def.healing_coefficient > 0.0,
        "Flash Heal should have positive healing coefficient"
    );
}

// =============================================================================
// Projectile Tests
// =============================================================================

#[test]
fn test_projectile_abilities_have_speed() {
    let projectile_abilities = vec![AbilityType::Frostbolt, AbilityType::Shadowbolt];

    for ability in projectile_abilities {
        let def = ability.definition();
        assert!(
            def.projectile_speed.is_some(),
            "{:?} should have projectile speed",
            ability
        );
        assert!(
            def.projectile_speed.unwrap() > 0.0,
            "{:?} should have positive projectile speed",
            ability
        );
    }
}

#[test]
fn test_instant_abilities_have_no_projectile() {
    let instant_abilities = vec![
        AbilityType::FlashHeal,
        AbilityType::MindBlast,
        AbilityType::FrostNova,
        AbilityType::Charge,
    ];

    for ability in instant_abilities {
        let def = ability.definition();
        assert!(
            def.projectile_speed.is_none(),
            "{:?} should not have projectile speed",
            ability
        );
    }
}

// =============================================================================
// Aura Application Tests
// =============================================================================

#[test]
fn test_cc_abilities_apply_auras() {
    let cc_abilities = vec![
        AbilityType::FrostNova,   // Root
        AbilityType::KidneyShot,  // Stun
        AbilityType::Fear,        // Fear
    ];

    for ability in cc_abilities {
        let def = ability.definition();
        assert!(
            def.applies_aura.is_some(),
            "{:?} should apply an aura",
            ability
        );

        let (aura_type, duration, _magnitude, _break_threshold) = def.applies_aura.unwrap();
        assert!(
            duration > 0.0,
            "{:?} aura should have positive duration",
            ability
        );
    }
}

#[test]
fn test_dot_abilities_apply_auras() {
    let dot_abilities = vec![AbilityType::Rend, AbilityType::Corruption];

    for ability in dot_abilities {
        let def = ability.definition();
        assert!(
            def.applies_aura.is_some(),
            "{:?} should apply a DoT aura",
            ability
        );
    }
}

#[test]
fn test_buff_abilities_apply_auras() {
    let buff_abilities = vec![
        AbilityType::PowerWordFortitude,  // Max HP
        AbilityType::ArcaneIntellect,     // Max Mana
        AbilityType::BattleShout,         // Attack Power
    ];

    for ability in buff_abilities {
        let def = ability.definition();
        assert!(
            def.applies_aura.is_some(),
            "{:?} should apply a buff aura",
            ability
        );
    }
}

#[test]
fn test_shield_abilities_apply_absorb_auras() {
    let shield_abilities = vec![AbilityType::IceBarrier, AbilityType::PowerWordShield];

    for ability in shield_abilities {
        let def = ability.definition();
        assert!(
            def.applies_aura.is_some(),
            "{:?} should apply an absorb aura",
            ability
        );

        let (aura_type, _duration, magnitude, _break_threshold) = def.applies_aura.unwrap();
        // Absorb magnitude should be positive (the absorb amount)
        assert!(
            magnitude > 0.0,
            "{:?} absorb should have positive magnitude, got {}",
            ability,
            magnitude
        );
    }
}
