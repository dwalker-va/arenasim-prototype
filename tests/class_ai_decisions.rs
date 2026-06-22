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

use std::collections::BTreeMap;

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
        velocity: Vec3::ZERO,
        is_alive: true,
        stealthed: false,
        target: None,
        is_pet: false,
        pet_type: None,
        pet: None,
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
    let mut combatants = BTreeMap::new();
    combatants.insert(self_entity, info(self_entity, team, class));
    CombatSnapshot {
        combatants,
        active_auras: BTreeMap::new(),
        dr_trackers: BTreeMap::new(),
        ability_cooldowns: BTreeMap::new(),
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

// ============================================================================
// Threat predicates (U4.2) — enemies_targeting / primary_attacker /
// attacker_escape_window / is_closing
// ============================================================================

/// Priest at origin, returns (snapshot, priest_entity). Tests extend it.
fn priest_snapshot() -> (CombatSnapshot, Entity) {
    let me = Entity::from_raw(1);
    (snapshot_for(me, 1, CharacterClass::Priest), me)
}

#[test]
fn enemies_targeting_excludes_stealthed_rogue() {
    let (mut snapshot, me) = priest_snapshot();
    let rogue = Entity::from_raw(2);
    snapshot.combatants.insert(rogue, CombatantInfo {
        stealthed: true,
        target: Some(me),
        position: Vec3::new(3.0, 0.0, 0.0),
        ..info(rogue, 2, CharacterClass::Rogue)
    });

    let ctx = snapshot.context_for(me);
    assert!(
        ctx.enemies_targeting(me).is_empty(),
        "a stealthed Rogue targeting me must NOT register as a threat"
    );
}

#[test]
fn enemies_targeting_includes_stealthed_rogue_under_shadow_sight() {
    let (mut snapshot, me) = priest_snapshot();
    let rogue = Entity::from_raw(2);
    snapshot.combatants.insert(rogue, CombatantInfo {
        stealthed: true,
        target: Some(me),
        position: Vec3::new(3.0, 0.0, 0.0),
        ..info(rogue, 2, CharacterClass::Rogue)
    });
    // I hold Shadow Sight — the stealthed Rogue is revealed.
    snapshot
        .active_auras
        .insert(me, vec![aura_with(AuraType::ShadowSight, None, -1.0)]);

    let ctx = snapshot.context_for(me);
    let threats = ctx.enemies_targeting(me);
    assert_eq!(threats.len(), 1, "shadow sight reveals the stealthed threat");
    assert_eq!(threats[0].entity, rogue);
}

#[test]
fn enemies_targeting_includes_rogue_holding_shadow_sight() {
    // The other arm of the can_see rule: an enemy that picked up Shadow
    // Sight is revealed even while stealthed.
    let (mut snapshot, me) = priest_snapshot();
    let rogue = Entity::from_raw(2);
    snapshot.combatants.insert(rogue, CombatantInfo {
        stealthed: true,
        target: Some(me),
        ..info(rogue, 2, CharacterClass::Rogue)
    });
    snapshot
        .active_auras
        .insert(rogue, vec![aura_with(AuraType::ShadowSight, None, -1.0)]);

    let ctx = snapshot.context_for(me);
    assert_eq!(ctx.enemies_targeting(me).len(), 1);
}

#[test]
fn enemies_targeting_includes_enemy_pet() {
    let (mut snapshot, me) = priest_snapshot();
    let pet = Entity::from_raw(2);
    snapshot.combatants.insert(pet, CombatantInfo {
        target: Some(me),
        position: Vec3::new(4.0, 0.0, 0.0),
        ..pet_info(pet, 2, CharacterClass::Warlock)
    });

    let ctx = snapshot.context_for(me);
    let threats = ctx.enemies_targeting(me);
    assert_eq!(threats.len(), 1, "enemy pets count as threats");
    assert_eq!(threats[0].entity, pet);
}

#[test]
fn enemies_targeting_excludes_enemy_targeting_someone_else_and_dead() {
    let (mut snapshot, me) = priest_snapshot();
    let ally = Entity::from_raw(2);
    snapshot.combatants.insert(ally, info(ally, 1, CharacterClass::Warrior));
    // Enemy on my team's Warrior, not me.
    let warrior = Entity::from_raw(3);
    snapshot.combatants.insert(warrior, CombatantInfo {
        target: Some(ally),
        ..info(warrior, 2, CharacterClass::Warrior)
    });
    // Dead enemy "targeting" me.
    let corpse = Entity::from_raw(4);
    snapshot.combatants.insert(corpse, CombatantInfo {
        target: Some(me),
        is_alive: false,
        current_health: 0.0,
        ..info(corpse, 2, CharacterClass::Rogue)
    });

    let ctx = snapshot.context_for(me);
    assert!(ctx.enemies_targeting(me).is_empty());
}

#[test]
fn primary_attacker_picks_nearest_of_two() {
    let (mut snapshot, me) = priest_snapshot();
    let far = Entity::from_raw(2);
    snapshot.combatants.insert(far, CombatantInfo {
        target: Some(me),
        position: Vec3::new(20.0, 0.0, 0.0),
        ..info(far, 2, CharacterClass::Warrior)
    });
    let near = Entity::from_raw(3);
    snapshot.combatants.insert(near, CombatantInfo {
        target: Some(me),
        position: Vec3::new(5.0, 0.0, 0.0),
        ..info(near, 2, CharacterClass::Rogue)
    });

    let ctx = snapshot.context_for(me);
    let attacker = ctx.primary_attacker(me).expect("two attackers exist");
    assert_eq!(attacker.entity, near, "nearest attacker wins");
}

#[test]
fn primary_attacker_skips_dead_and_invisible() {
    let (mut snapshot, me) = priest_snapshot();
    // Nearest is dead.
    let dead = Entity::from_raw(2);
    snapshot.combatants.insert(dead, CombatantInfo {
        target: Some(me),
        position: Vec3::new(2.0, 0.0, 0.0),
        is_alive: false,
        current_health: 0.0,
        ..info(dead, 2, CharacterClass::Warrior)
    });
    // Second-nearest is stealthed (invisible).
    let hidden = Entity::from_raw(3);
    snapshot.combatants.insert(hidden, CombatantInfo {
        target: Some(me),
        position: Vec3::new(4.0, 0.0, 0.0),
        stealthed: true,
        ..info(hidden, 2, CharacterClass::Rogue)
    });
    // Farthest is the only live, visible attacker.
    let live = Entity::from_raw(4);
    snapshot.combatants.insert(live, CombatantInfo {
        target: Some(me),
        position: Vec3::new(15.0, 0.0, 0.0),
        ..info(live, 2, CharacterClass::Warrior)
    });

    let ctx = snapshot.context_for(me);
    let attacker = ctx.primary_attacker(me).expect("one valid attacker");
    assert_eq!(attacker.entity, live);
}

#[test]
fn primary_attacker_none_when_unthreatened() {
    let (mut snapshot, me) = priest_snapshot();
    let enemy = Entity::from_raw(2);
    snapshot.combatants.insert(enemy, info(enemy, 2, CharacterClass::Mage)); // target: None

    let ctx = snapshot.context_for(me);
    assert!(ctx.primary_attacker(me).is_none());
}

#[test]
fn attacker_escape_window_returns_remaining_impair_duration() {
    let (mut snapshot, me) = priest_snapshot();
    let attacker = Entity::from_raw(2);
    snapshot.combatants.insert(attacker, CombatantInfo {
        target: Some(me),
        ..info(attacker, 2, CharacterClass::Warrior)
    });

    for (effect, expected) in [
        (AuraType::Root, 5.0_f32),
        (AuraType::Stun, 5.0),
        (AuraType::Incapacitate, 5.0),
    ] {
        snapshot
            .active_auras
            .insert(attacker, vec![aura_with(effect, None, -1.0)]);
        let ctx = snapshot.context_for(me);
        let window = ctx.attacker_escape_window(attacker);
        assert_eq!(
            window,
            Some(expected),
            "{:?} must open an escape window of its remaining duration",
            effect
        );
    }
}

#[test]
fn attacker_escape_window_takes_longest_of_multiple() {
    let (mut snapshot, me) = priest_snapshot();
    let attacker = Entity::from_raw(2);
    snapshot.combatants.insert(attacker, info(attacker, 2, CharacterClass::Warrior));

    let mut short_stun = aura_with(AuraType::Stun, None, -1.0);
    short_stun.duration = 1.5;
    let mut long_root = aura_with(AuraType::Root, None, 80.0);
    long_root.duration = 6.0;
    snapshot.active_auras.insert(attacker, vec![short_stun, long_root]);

    let ctx = snapshot.context_for(me);
    assert_eq!(ctx.attacker_escape_window(attacker), Some(6.0));
}

#[test]
fn attacker_escape_window_none_for_fear_or_free_attacker() {
    let (mut snapshot, me) = priest_snapshot();
    let attacker = Entity::from_raw(2);
    snapshot.combatants.insert(attacker, info(attacker, 2, CharacterClass::Warrior));

    // No CC at all → no window.
    {
        let ctx = snapshot.context_for(me);
        assert_eq!(ctx.attacker_escape_window(attacker), None);
    }

    // Fear is excluded — it self-solves (the attacker wanders off).
    snapshot
        .active_auras
        .insert(attacker, vec![aura_with(AuraType::Fear, None, 80.0)]);
    let ctx = snapshot.context_for(me);
    assert_eq!(ctx.attacker_escape_window(attacker), None);
}

#[test]
fn is_closing_true_for_melee_pursuing_me() {
    let (mut snapshot, me) = priest_snapshot();
    // Warrior (preferred_range 2.0) at 10 units, kill target = me: its
    // pursuit moves toward me this frame.
    let warrior = Entity::from_raw(2);
    snapshot.combatants.insert(warrior, CombatantInfo {
        target: Some(me),
        position: Vec3::new(10.0, 0.0, 0.0),
        ..info(warrior, 2, CharacterClass::Warrior)
    });

    let ctx = snapshot.context_for(me);
    assert!(ctx.is_closing(warrior, me));
}

#[test]
fn is_closing_false_for_stationary_caster_in_range() {
    let (mut snapshot, me) = priest_snapshot();
    // Mage (preferred_range 38.0) at 20 units targeting me: already inside
    // its preferred range, so pursuit holds position — not closing.
    let mage = Entity::from_raw(2);
    snapshot.combatants.insert(mage, CombatantInfo {
        target: Some(me),
        position: Vec3::new(20.0, 0.0, 0.0),
        ..info(mage, 2, CharacterClass::Mage)
    });

    let ctx = snapshot.context_for(me);
    assert!(!ctx.is_closing(mage, me));
}

#[test]
fn is_closing_false_when_threat_targets_someone_else_or_is_in_melee() {
    let (mut snapshot, me) = priest_snapshot();
    let ally = Entity::from_raw(2);
    snapshot.combatants.insert(ally, info(ally, 1, CharacterClass::Warrior));

    // Distant melee whose kill target is my ally, not me.
    let off_target = Entity::from_raw(3);
    snapshot.combatants.insert(off_target, CombatantInfo {
        target: Some(ally),
        position: Vec3::new(10.0, 0.0, 0.0),
        ..info(off_target, 2, CharacterClass::Warrior)
    });
    // Melee already on top of me (inside preferred_range 2.0): targeting me
    // but not "closing" — it is already there.
    let in_melee = Entity::from_raw(4);
    snapshot.combatants.insert(in_melee, CombatantInfo {
        target: Some(me),
        position: Vec3::new(1.5, 0.0, 0.0),
        ..info(in_melee, 2, CharacterClass::Rogue)
    });

    let ctx = snapshot.context_for(me);
    assert!(!ctx.is_closing(off_target, me));
    assert!(!ctx.is_closing(in_melee, me));
}

#[test]
fn is_closing_uses_pet_preferred_range_for_pets() {
    let (mut snapshot, me) = priest_snapshot();
    // Felhunter (pet preferred_range 2.0, melee) at 12 units with kill
    // target me: closing.
    let felhunter = Entity::from_raw(2);
    snapshot.combatants.insert(felhunter, CombatantInfo {
        target: Some(me),
        position: Vec3::new(12.0, 0.0, 0.0),
        ..pet_info(felhunter, 2, CharacterClass::Warlock)
    });

    let ctx = snapshot.context_for(me);
    assert!(ctx.is_closing(felhunter, me));
}

// ============================================================================
// visible_enemies_within — proximity threat half of the PRESSURED trigger
// ============================================================================

#[test]
fn visible_enemies_within_includes_only_enemies_inside_radius() {
    let (mut snapshot, me) = priest_snapshot();
    // Enemy at distance 5 (inside radius 10).
    let near = Entity::from_raw(2);
    snapshot.combatants.insert(near, CombatantInfo {
        position: Vec3::new(5.0, 0.0, 0.0),
        ..info(near, 2, CharacterClass::Warrior)
    });
    // Enemy at distance 15 (outside radius 10).
    let far = Entity::from_raw(3);
    snapshot.combatants.insert(far, CombatantInfo {
        position: Vec3::new(15.0, 0.0, 0.0),
        ..info(far, 2, CharacterClass::Mage)
    });

    let ctx = snapshot.context_for(me);
    let within: Vec<Entity> = ctx
        .visible_enemies_within(me, Vec3::ZERO, 10.0)
        .iter()
        .map(|c| c.entity)
        .collect();
    assert_eq!(within, vec![near], "only the enemy inside the radius is returned");
}

#[test]
fn visible_enemies_within_respects_radius_boundary_and_team() {
    let (mut snapshot, me) = priest_snapshot();
    // Enemy exactly at the radius (10.0) — `<=` so it is included.
    let on_edge = Entity::from_raw(2);
    snapshot.combatants.insert(on_edge, CombatantInfo {
        position: Vec3::new(10.0, 0.0, 0.0),
        ..info(on_edge, 2, CharacterClass::Warrior)
    });
    // Ally inside the radius — never a "threat", regardless of distance.
    let ally = Entity::from_raw(3);
    snapshot.combatants.insert(ally, CombatantInfo {
        position: Vec3::new(2.0, 0.0, 0.0),
        ..info(ally, 1, CharacterClass::Warrior)
    });

    let ctx = snapshot.context_for(me);
    let within: Vec<Entity> = ctx
        .visible_enemies_within(me, Vec3::ZERO, 10.0)
        .iter()
        .map(|c| c.entity)
        .collect();
    assert_eq!(within, vec![on_edge], "boundary enemy in, ally out");
}

// ============================================================================
// movement_slow_multiplier — product of MovementSpeedSlow magnitudes
// ============================================================================

#[test]
fn movement_slow_multiplier_no_aura_is_one() {
    let (snapshot, me) = priest_snapshot();
    let ctx = snapshot.context_for(me);
    assert_eq!(ctx.movement_slow_multiplier(me), 1.0, "unslowed = 1.0");
}

#[test]
fn movement_slow_multiplier_single_slow() {
    let (mut snapshot, me) = priest_snapshot();
    let mut slow = aura_with(AuraType::MovementSpeedSlow, None, 0.0);
    slow.magnitude = 0.5;
    snapshot.active_auras.insert(me, vec![slow]);

    let ctx = snapshot.context_for(me);
    assert_eq!(ctx.movement_slow_multiplier(me), 0.5, "one 50% slow halves speed");
}

#[test]
fn movement_slow_multiplier_stacks_multiplicatively() {
    let (mut snapshot, me) = priest_snapshot();
    let mut slow_a = aura_with(AuraType::MovementSpeedSlow, None, 0.0);
    slow_a.magnitude = 0.5;
    let mut slow_b = aura_with(AuraType::MovementSpeedSlow, None, 0.0);
    slow_b.magnitude = 0.7;
    // A non-slow aura must be ignored by the product.
    let unrelated = aura_with(AuraType::DamageOverTime, None, 0.0);
    snapshot.active_auras.insert(me, vec![slow_a, slow_b, unrelated]);

    let ctx = snapshot.context_for(me);
    // 0.5 * 0.7 = 0.35 (the DoT does not participate).
    assert!(
        (ctx.movement_slow_multiplier(me) - 0.35).abs() < 1e-6,
        "two slows multiply: got {}",
        ctx.movement_slow_multiplier(me)
    );
}
