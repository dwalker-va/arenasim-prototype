//! Isolated tests for class-AI decision predicates.
//!
//! Every predicate here lives on `CombatContext` (or as a free helper in
//! `class_ai::mod`) and is read by ~10-30 sites across the seven class AI
//! modules. End-to-end coverage via headless matches is too coarse to
//! catch a regression in any single rule, so we exercise each predicate
//! against a hand-built `CombatSnapshot`.
//!
//! Construction is cheap because PR #45 made `CombatSnapshot` a plain
//! struct with public fields; no Bevy world is needed.

use std::collections::HashMap;

use bevy::prelude::*;

use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::class_ai::combat_snapshot::CombatSnapshot;
use arenasim::states::play_match::class_ai::{dispel_priority, CombatantInfo};
use arenasim::states::play_match::{Aura, AuraType, DRCategory, DRTracker, PetType};

// ============================================================================
// Fixture helpers
// ============================================================================

fn info(entity: Entity, team: u8, class: CharacterClass) -> CombatantInfo {
    CombatantInfo {
        entity,
        team,
        slot: 0,
        class,
        current_health: 100.0,
        max_health: 100.0,
        current_mana: 100.0,
        max_mana: 100.0,
        position: Vec3::ZERO,
        is_alive: true,
        stealthed: false,
        target: None,
        is_pet: false,
        pet_type: None,
    }
}

fn pet_info(entity: Entity, team: u8, owner_class: CharacterClass) -> CombatantInfo {
    CombatantInfo {
        is_pet: true,
        pet_type: Some(PetType::Felhunter),
        ..info(entity, team, owner_class)
    }
}

fn aura_with(effect_type: AuraType, caster: Option<Entity>, break_on_damage_threshold: f32) -> Aura {
    Aura {
        effect_type,
        duration: 5.0,
        magnitude: 1.0,
        break_on_damage_threshold,
        accumulated_damage: 0.0,
        tick_interval: 0.0,
        time_until_next_tick: 0.0,
        caster,
        ability_name: format!("{:?}", effect_type),
        fear_direction: (0.0, 0.0),
        fear_direction_timer: 0.0,
        spell_school: None,
        applied_this_frame: false,
        backlash_damage: None,
    }
}

/// Empty snapshot with the caster registered as the self-entity. Tests
/// extend `combatants`, `active_auras`, and `dr_trackers` as they need.
fn snapshot_for(self_entity: Entity, team: u8, class: CharacterClass) -> CombatSnapshot {
    let mut combatants = HashMap::new();
    combatants.insert(self_entity, info(self_entity, team, class));
    CombatSnapshot {
        combatants,
        active_auras: HashMap::new(),
        dr_trackers: HashMap::new(),
    }
}

// ============================================================================
// dispel_priority — table ordering
// ============================================================================

#[test]
fn dispel_priority_orders_cc_above_dots_above_slows() {
    // Healers prefer dispelling Polymorph over Fear over Root over DoTs over slows.
    // A regression that swapped any two of these would pass `cargo test` today
    // because no isolated test guarded the order.
    assert!(dispel_priority(AuraType::Polymorph) > dispel_priority(AuraType::Fear));
    assert!(dispel_priority(AuraType::Fear) > dispel_priority(AuraType::Root));
    assert!(dispel_priority(AuraType::Root) > dispel_priority(AuraType::DamageOverTime));
    assert!(dispel_priority(AuraType::DamageOverTime) > dispel_priority(AuraType::MovementSpeedSlow));
    assert!(dispel_priority(AuraType::MovementSpeedSlow) > 0);
}

#[test]
fn dispel_priority_returns_zero_for_buffs() {
    // Beneficial auras and non-dispellable effects shouldn't score above
    // anything `try_dispel_ally` considers actionable (min_priority >= 20).
    assert_eq!(dispel_priority(AuraType::Absorb), 0);
    assert_eq!(dispel_priority(AuraType::AttackPowerIncrease), 0);
    assert_eq!(dispel_priority(AuraType::WeakenedSoul), 0);
}

// ============================================================================
// has_friendly_breakable_cc — BUG-1 guard
// ============================================================================

#[test]
fn has_friendly_breakable_cc_detects_team_polymorph() {
    let me = Entity::from_raw(1);
    let ally = Entity::from_raw(2);
    let enemy = Entity::from_raw(3);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Warlock);
    snapshot.combatants.insert(ally, info(ally, 1, CharacterClass::Mage));
    snapshot.combatants.insert(enemy, info(enemy, 2, CharacterClass::Warrior));

    // Ally Polymorphed the enemy — break_on_damage_threshold == 0.0 means it
    // breaks on any damage, which is the signal `has_friendly_breakable_cc`
    // looks for.
    snapshot.active_auras.insert(enemy, vec![aura_with(AuraType::Polymorph, Some(ally), 0.0)]);

    let ctx = snapshot.context_for(me);
    assert!(ctx.has_friendly_breakable_cc(enemy));
}

#[test]
fn has_friendly_breakable_cc_ignores_enemy_caster() {
    let me = Entity::from_raw(1);
    let enemy_caster = Entity::from_raw(2);
    let target = Entity::from_raw(3);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Warrior);
    snapshot.combatants.insert(enemy_caster, info(enemy_caster, 2, CharacterClass::Mage));
    snapshot.combatants.insert(target, info(target, 2, CharacterClass::Priest));

    // Enemy mage Polymorphed their own teammate. Not our problem — we can
    // damage that target without breaking *our* CC.
    snapshot.active_auras.insert(target, vec![aura_with(AuraType::Polymorph, Some(enemy_caster), 0.0)]);

    let ctx = snapshot.context_for(me);
    assert!(!ctx.has_friendly_breakable_cc(target));
}

#[test]
fn has_friendly_breakable_cc_ignores_high_threshold_auras() {
    let me = Entity::from_raw(1);
    let ally = Entity::from_raw(2);
    let enemy = Entity::from_raw(3);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Warrior);
    snapshot.combatants.insert(ally, info(ally, 1, CharacterClass::Mage));
    snapshot.combatants.insert(enemy, info(enemy, 2, CharacterClass::Rogue));

    // Frost Nova root: break_on_damage_threshold == 80.0, not 0.0. It absorbs
    // moderate damage before breaking, so attacking the target is fine.
    snapshot.active_auras.insert(enemy, vec![aura_with(AuraType::Root, Some(ally), 80.0)]);

    let ctx = snapshot.context_for(me);
    assert!(!ctx.has_friendly_breakable_cc(enemy));
}

// ============================================================================
// has_friendly_dots_on_target — BUG-2 (Polymorph onto a friendly DoT)
// ============================================================================

#[test]
fn has_friendly_dots_on_target_detects_team_dot() {
    let me = Entity::from_raw(1); // mage
    let ally = Entity::from_raw(2); // warlock
    let enemy = Entity::from_raw(3);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Mage);
    snapshot.combatants.insert(ally, info(ally, 1, CharacterClass::Warlock));
    snapshot.combatants.insert(enemy, info(enemy, 2, CharacterClass::Priest));

    // Warlock teammate has Corruption ticking. Polymorph would break next tick.
    snapshot.active_auras.insert(enemy, vec![aura_with(AuraType::DamageOverTime, Some(ally), -1.0)]);

    let ctx = snapshot.context_for(me);
    assert!(ctx.has_friendly_dots_on_target(enemy));
}

#[test]
fn has_friendly_dots_on_target_ignores_enemy_dot() {
    let me = Entity::from_raw(1);
    let enemy_warlock = Entity::from_raw(2);
    let teammate = Entity::from_raw(3);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Mage);
    snapshot.combatants.insert(enemy_warlock, info(enemy_warlock, 2, CharacterClass::Warlock));
    snapshot.combatants.insert(teammate, info(teammate, 1, CharacterClass::Priest));

    // The DoT here is on a teammate, applied by an enemy — irrelevant to
    // whether *we* would break our own CC by Polymorphing the *target*.
    snapshot.active_auras.insert(teammate, vec![aura_with(AuraType::DamageOverTime, Some(enemy_warlock), -1.0)]);

    let ctx = snapshot.context_for(me);
    assert!(!ctx.has_friendly_dots_on_target(teammate));
}

// ============================================================================
// lowest_health_ally_below — used by every healer's try_*
// ============================================================================

#[test]
fn lowest_health_ally_below_returns_lowest_under_threshold() {
    let me = Entity::from_raw(1);
    let ally_high = Entity::from_raw(2);
    let ally_low = Entity::from_raw(3);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Priest);
    let mut high = info(ally_high, 1, CharacterClass::Warrior);
    high.current_health = 80.0;
    let mut low = info(ally_low, 1, CharacterClass::Mage);
    low.current_health = 30.0;
    snapshot.combatants.insert(ally_high, high);
    snapshot.combatants.insert(ally_low, low);

    let ctx = snapshot.context_for(me);
    let target = ctx.lowest_health_ally_below(0.9, f32::MAX, Vec3::ZERO).expect("ally below 90%");
    assert_eq!(target.entity, ally_low);
}

#[test]
fn lowest_health_ally_below_excludes_pets() {
    let me = Entity::from_raw(1);
    let ally = Entity::from_raw(2);
    let pet = Entity::from_raw(3);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Priest);
    let mut ally_info = info(ally, 1, CharacterClass::Warrior);
    ally_info.current_health = 80.0;
    let mut pet_inf = pet_info(pet, 1, CharacterClass::Hunter);
    pet_inf.current_health = 10.0; // very low — but it's a pet
    snapshot.combatants.insert(ally, ally_info);
    snapshot.combatants.insert(pet, pet_inf);

    let ctx = snapshot.context_for(me);
    let target = ctx.lowest_health_ally_below(0.9, f32::MAX, Vec3::ZERO).expect("non-pet ally");
    assert_eq!(target.entity, ally, "pet must not be returned even though its HP is lowest");
}

#[test]
fn lowest_health_ally_below_respects_range() {
    let me = Entity::from_raw(1);
    let near = Entity::from_raw(2);
    let far = Entity::from_raw(3);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Priest);
    let mut near_info = info(near, 1, CharacterClass::Warrior);
    near_info.current_health = 80.0;
    near_info.position = Vec3::new(5.0, 0.0, 0.0);
    let mut far_info = info(far, 1, CharacterClass::Mage);
    far_info.current_health = 10.0; // lower HP, but out of range
    far_info.position = Vec3::new(50.0, 0.0, 0.0);
    snapshot.combatants.insert(near, near_info);
    snapshot.combatants.insert(far, far_info);

    let ctx = snapshot.context_for(me);
    // Healing range = 30 units. The far ally is closer to dead but we cannot reach them.
    let target = ctx.lowest_health_ally_below(0.9, 30.0, Vec3::ZERO).expect("near ally");
    assert_eq!(target.entity, near);
}

#[test]
fn lowest_health_ally_below_returns_none_when_team_is_full_hp() {
    let me = Entity::from_raw(1);
    let ally = Entity::from_raw(2);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Priest);
    snapshot.combatants.insert(ally, info(ally, 1, CharacterClass::Warrior));

    let ctx = snapshot.context_for(me);
    // Threshold 0.9 — nobody is below it (self + ally are both at 100%).
    assert!(ctx.lowest_health_ally_below(0.9, f32::MAX, Vec3::ZERO).is_none());
    assert!(ctx.is_team_healthy(0.9, Vec3::ZERO));
}

// ============================================================================
// is_ccd — used by every CC ability to avoid stacking
// ============================================================================

#[test]
fn is_ccd_detects_each_hard_cc_type() {
    let me = Entity::from_raw(1);
    let target = Entity::from_raw(2);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Mage);
    snapshot.combatants.insert(target, info(target, 2, CharacterClass::Warrior));

    // Each of these aura types should make the target read as CC'd.
    for cc in [
        AuraType::Stun,
        AuraType::Fear,
        AuraType::Root,
        AuraType::Polymorph,
        AuraType::Incapacitate,
    ] {
        snapshot.active_auras.insert(target, vec![aura_with(cc, None, -1.0)]);
        let ctx = snapshot.context_for(me);
        assert!(ctx.is_ccd(target), "is_ccd should return true for {:?}", cc);
    }
}

#[test]
fn is_ccd_returns_false_for_non_cc_auras_and_missing_target() {
    let me = Entity::from_raw(1);
    let target = Entity::from_raw(2);
    let unknown = Entity::from_raw(99);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Mage);
    snapshot.combatants.insert(target, info(target, 2, CharacterClass::Warrior));

    // DoT is a debuff but not CC.
    snapshot.active_auras.insert(target, vec![aura_with(AuraType::DamageOverTime, None, -1.0)]);
    let ctx = snapshot.context_for(me);
    assert!(!ctx.is_ccd(target), "DoT is not CC");

    // Entity not in the snapshot at all (e.g. mid-frame target lookup miss).
    assert!(!ctx.is_ccd(unknown));
}

// ============================================================================
// is_dr_immune — used by AI to avoid wasting CCs into DR walls
// ============================================================================

#[test]
fn is_dr_immune_returns_false_when_no_tracker() {
    let me = Entity::from_raw(1);
    let target = Entity::from_raw(2);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Rogue);
    snapshot.combatants.insert(target, info(target, 2, CharacterClass::Priest));
    // No DRTracker entry — target has never been CC'd.

    let ctx = snapshot.context_for(me);
    assert!(!ctx.is_dr_immune(target, DRCategory::Stuns));
}

#[test]
fn is_dr_immune_returns_true_after_three_stuns() {
    let me = Entity::from_raw(1);
    let target = Entity::from_raw(2);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Rogue);
    snapshot.combatants.insert(target, info(target, 2, CharacterClass::Priest));

    // Drive the Stuns category to immunity (DR ladder: 100% → 50% → 25% → immune).
    let mut tracker = DRTracker::default();
    tracker.apply(DRCategory::Stuns);
    tracker.apply(DRCategory::Stuns);
    tracker.apply(DRCategory::Stuns);
    snapshot.dr_trackers.insert(target, tracker);

    let ctx = snapshot.context_for(me);
    assert!(ctx.is_dr_immune(target, DRCategory::Stuns));
    // Different categories share no DR — Incapacitates is still actionable.
    assert!(!ctx.is_dr_immune(target, DRCategory::Incapacitates));
}
