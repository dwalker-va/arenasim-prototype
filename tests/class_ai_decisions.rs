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
use arenasim::states::play_match::class_ai::{
    dispel_priority, purge_priority, team_hp_sums, CombatantInfo, PURGE_MIN_PRIORITY,
};
use arenasim::states::play_match::{Aura, AuraType, DRCategory, DRTracker, DispelType, PetType};

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
        casting_ability: None,
        pet_type: None,
        pet: None,
    }
}

fn pet_info(entity: Entity, team: u8, owner_class: CharacterClass) -> CombatantInfo {
    CombatantInfo {
        is_pet: true,
        casting_ability: None,
        pet_type: Some(PetType::Felhunter),
        ..info(entity, team, owner_class)
    }
}

fn aura_with(
    effect_type: AuraType,
    caster: Option<Entity>,
    break_on_damage_threshold: f32,
) -> Aura {
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
        dr_category_override: None,
        dispel_type: DispelType::Auto,
        compound: None,
        source_item: None,
        stacks: None,
        bloom: None,
    }
}

/// Empty snapshot with the caster registered as the self-entity. Tests
/// extend `combatants`, `active_auras`, and `dr_trackers` as they need.
fn snapshot_for(self_entity: Entity, team: u8, class: CharacterClass) -> CombatSnapshot {
    let mut combatants = BTreeMap::new();
    combatants.insert(self_entity, info(self_entity, team, class));
    CombatSnapshot {
        ai_profile: Default::default(),
        bounds: Default::default(),
        combatants,
        active_auras: BTreeMap::new(),
        dr_trackers: BTreeMap::new(),
        ability_cooldowns: BTreeMap::new(),
        obstacles: Vec::new(),
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
    assert!(
        dispel_priority(AuraType::DamageOverTime) > dispel_priority(AuraType::MovementSpeedSlow)
    );
    assert!(dispel_priority(AuraType::MovementSpeedSlow) > 0);
}

/// AS-44 regression. `try_dispel_ally` is called with exactly two
/// `min_priority` bars — 90 (urgent) and 50 (maintenance) — so a dispellable
/// aura scoring below 90 is invisible to a pressured healer and one scoring 0
/// is invisible to every healer, forever. Freezing Trap's `Incapacitate` and
/// Unstable Affliction's backlash `Silence` both sat at 0 behind a `_ => 0`
/// wildcard: classified dispellable, removal path wired, never once removed
/// across 32 seeded matches carrying 19 dispels and 46 cleanses.
///
/// Swept over `AuraType::ALL`, so the assertion is two-sided: the four urgent
/// types must clear the bar, and NOTHING ELSE may — a variant promoted into the
/// urgent band is a decision about what a pressured healer drops a heal for,
/// and it should be made here rather than noticed later.
#[test]
fn urgent_crowd_control_clears_every_caller_bar() {
    const URGENT_BAR: i32 = 90;
    const URGENT: [AuraType; 4] = [
        AuraType::Polymorph,
        AuraType::Incapacitate, // Freezing Trap
        AuraType::Silence,      // Unstable Affliction backlash
        AuraType::Fear,
    ];
    for ty in AuraType::ALL {
        let expected = URGENT.contains(&ty);
        assert_eq!(
            dispel_priority(ty) >= URGENT_BAR,
            expected,
            "{ty:?} scored {} against the 90 urgent bar, but it is {}",
            dispel_priority(ty),
            if expected {
                "urgent crowd control"
            } else {
                "not urgent crowd control"
            }
        );
    }
}

/// Ordering intent for the two types AS-44 promoted: the complete
/// incapacitates tie at the top, the silence sits between them and Fear.
#[test]
fn dispel_priority_ranks_incapacitates_top_and_silence_above_fear() {
    assert_eq!(
        dispel_priority(AuraType::Incapacitate),
        dispel_priority(AuraType::Polymorph),
        "Freezing Trap and Polymorph are both complete incapacitates sharing a \
         DR category — neither outranks the other"
    );
    assert!(dispel_priority(AuraType::Polymorph) > dispel_priority(AuraType::Silence));
    assert!(dispel_priority(AuraType::Silence) > dispel_priority(AuraType::Fear));
}

/// Structural guard: a type the dispel path can actually remove must be GRADED.
/// A magic-dispellable type at 0 is unreachable by every caller — a debuff no
/// healer will ever lift, which is a balance decision, not a default.
///
/// The domain is `AuraType::ALL`, not a list written out here: a hand-kept copy
/// of the dispellable set would quietly stop covering whatever it forgot while
/// still reading as a whole-enum sweep. `is_magic_dispellable` picks the subset.
#[test]
fn every_magic_dispellable_type_is_graded() {
    let mut swept = 0;
    for ty in AuraType::ALL {
        if !ty.is_magic_dispellable() {
            continue;
        }
        swept += 1;
        assert!(
            dispel_priority(ty) > 0,
            "{ty:?} is dispellable but scores 0 — no caller's min_priority can ever reach it"
        );
    }
    // Non-vacuity: the sweep is only worth anything while the filter admits
    // something. Six types are dispellable today (the five CC mechanics plus
    // the Unstable Affliction Silence); a refactor that emptied the filter
    // would otherwise turn this test green by covering nothing.
    assert!(
        swept >= 6,
        "only {swept} magic-dispellable types found — the filter, not the grading, \
         is what changed"
    );
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
// purge_priority — table ordering (Shaman offensive dispel)
// ============================================================================

#[test]
fn purge_priority_orders_defensives_above_offensive_buffs() {
    // The Shaman strips the most valuable enemy buff first: heavy defensive
    // mitigation/sustain (Absorb, DamageTakenReduction, HoT) outrank offensive
    // throughput (AP/SP/Windfury/Crit), which outrank minor utility.
    // The headline invariant the rotation leans on: a defensive Absorb (PW:S)
    // outranks an AttackPower buff.
    assert!(purge_priority(AuraType::Absorb) > purge_priority(AuraType::AttackPowerIncrease));
    assert!(
        purge_priority(AuraType::DamageTakenReduction)
            > purge_priority(AuraType::AttackPowerIncrease)
    );
    assert!(
        purge_priority(AuraType::HealingOverTime) > purge_priority(AuraType::CritChanceIncrease)
    );
    assert!(
        purge_priority(AuraType::AttackPowerIncrease)
            > purge_priority(AuraType::LockoutDurationReduction)
    );
    assert!(purge_priority(AuraType::LockoutDurationReduction) > 0);
    // SP and AP are equal-value throughput buffs.
    assert_eq!(
        purge_priority(AuraType::SpellPowerIncrease),
        purge_priority(AuraType::AttackPowerIncrease)
    );

    // Lever C: PW:Fortitude (MaxHealthIncrease) is a CHEAP re-buff, deliberately
    // deprioritized below the offensive buffs AND below the action floor, so the
    // Shaman never wastes a GCD stripping it (the enemy just re-casts it).
    assert!(purge_priority(AuraType::MaxHealthIncrease) < purge_priority(AuraType::WindfuryBuff));
    assert!(purge_priority(AuraType::MaxHealthIncrease) < PURGE_MIN_PRIORITY);

    // The action floor only admits the heavy defensives (denying mitigation /
    // sustain is worth a cast); throughput re-buffs are below it.
    assert!(purge_priority(AuraType::Absorb) >= PURGE_MIN_PRIORITY);
    assert!(purge_priority(AuraType::DamageTakenReduction) >= PURGE_MIN_PRIORITY);
    assert!(purge_priority(AuraType::HealingOverTime) >= PURGE_MIN_PRIORITY);
    assert!(purge_priority(AuraType::AttackPowerIncrease) < PURGE_MIN_PRIORITY);
}

/// Sibling of `every_magic_dispellable_type_is_graded`, flagged by the AS-41
/// Tester: a purgeable buff scoring 0 falls below `PURGE_MIN_PRIORITY` and is
/// never purged by any AI, while reading as fully wired — the aura IS
/// `can_be_purged` and the removal path DOES exist.
///
/// Same shape as `every_magic_dispellable_type_is_graded`: the domain is
/// `AuraType::ALL` and `Aura::can_be_purged` picks the subset, so the test
/// cannot drift into covering less than it claims.
#[test]
fn every_purgeable_type_is_graded() {
    let mut swept = 0;
    for ty in AuraType::ALL {
        if !aura_with(ty, None, -1.0).can_be_purged() {
            continue;
        }
        swept += 1;
        assert!(
            purge_priority(ty) > 0,
            "{ty:?} is purgeable but scores 0 — the Shaman can never reach it"
        );
    }
    // Non-vacuity: 13 buffs are purgeable today.
    assert!(
        swept >= 13,
        "only {swept} purgeable types found — the filter, not the grading, is \
         what changed"
    );
}

/// The complement of `every_purgeable_type_is_graded`, swept over the same
/// domain so the two together account for every variant: debuffs and
/// un-purgeable markers/immunities are never purge targets, so they must score
/// 0 (below anything `try_purge_enemy` will act on).
#[test]
fn purge_priority_returns_zero_for_debuffs_and_unpurgeable() {
    for ty in AuraType::ALL {
        if aura_with(ty, None, -1.0).can_be_purged() {
            continue;
        }
        assert_eq!(
            purge_priority(ty),
            0,
            "{ty:?} is not purgeable, so it must not be a purge target"
        );
    }
}

// ============================================================================
// Shaman Purge target selection — mirrors try_purge_enemy's inner pick
// ============================================================================

/// Replicates `try_purge_enemy`'s candidate scan using the SAME production
/// predicates it uses (`Aura::can_be_purged` + `purge_priority`): among living
/// non-pet enemies, pick the enemy whose highest-priority purgeable buff is the
/// most valuable, returning `(enemy, chosen buff)`. `None` when no enemy carries
/// a purgeable buff (the reject path).
fn select_purge_target(snapshot: &CombatSnapshot, my_team: u8) -> Option<(Entity, AuraType)> {
    let mut best: Option<(Entity, AuraType, i32)> = None;
    for (e, inf) in snapshot.combatants.iter() {
        if inf.team == my_team || !inf.is_alive || inf.is_pet {
            continue;
        }
        let Some(auras) = snapshot.active_auras.get(e) else {
            continue;
        };
        for a in auras {
            if !a.can_be_purged() {
                continue;
            }
            let p = purge_priority(a.effect_type);
            match best {
                None => best = Some((*e, a.effect_type, p)),
                Some((_, _, bp)) if p > bp => best = Some((*e, a.effect_type, p)),
                _ => {}
            }
        }
    }
    best.map(|(e, a, _)| (e, a))
}

#[test]
fn shaman_purges_enemy_with_dispellable_buff() {
    let me = Entity::from_raw(1);
    let enemy = Entity::from_raw(2);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Shaman);
    snapshot
        .combatants
        .insert(enemy, info(enemy, 2, CharacterClass::Warrior));

    // Enemy carries an offensive AttackPower buff AND a defensive Absorb. Both
    // are purgeable; the Shaman strips the higher-priority defensive first.
    snapshot.active_auras.insert(
        enemy,
        vec![
            aura_with(AuraType::AttackPowerIncrease, None, -1.0),
            aura_with(AuraType::Absorb, None, -1.0),
        ],
    );

    let (target, buff) = select_purge_target(&snapshot, 1).expect("a purgeable enemy buff exists");
    assert_eq!(target, enemy, "the buffed enemy is the purge target");
    assert_eq!(
        buff,
        AuraType::Absorb,
        "the defensive Absorb outranks the AttackPower buff"
    );
}

#[test]
fn shaman_purge_finds_no_target_without_a_purgeable_buff() {
    let me = Entity::from_raw(1);
    let enemy = Entity::from_raw(2);
    let ally = Entity::from_raw(3);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Shaman);
    snapshot
        .combatants
        .insert(enemy, info(enemy, 2, CharacterClass::Warrior));
    snapshot
        .combatants
        .insert(ally, info(ally, 1, CharacterClass::Priest));

    // The enemy carries only a DEBUFF (a Root we put on it) — not purgeable.
    snapshot
        .active_auras
        .insert(enemy, vec![aura_with(AuraType::Root, None, -1.0)]);
    // Our ally carries a real buff (Absorb) — but Purge never strips allies.
    snapshot
        .active_auras
        .insert(ally, vec![aura_with(AuraType::Absorb, None, -1.0)]);

    assert!(
        select_purge_target(&snapshot, 1).is_none(),
        "no enemy carries a purgeable buff — Purge must find no target (the \
         ally's Absorb is off-limits)"
    );
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
    snapshot
        .combatants
        .insert(ally, info(ally, 1, CharacterClass::Mage));
    snapshot
        .combatants
        .insert(enemy, info(enemy, 2, CharacterClass::Warrior));

    // Ally Polymorphed the enemy — break_on_damage_threshold == 0.0 means it
    // breaks on any damage, which is the signal `has_friendly_breakable_cc`
    // looks for.
    snapshot
        .active_auras
        .insert(enemy, vec![aura_with(AuraType::Polymorph, Some(ally), 0.0)]);

    let ctx = snapshot.context_for(me);
    assert!(ctx.has_friendly_breakable_cc(enemy));
}

#[test]
fn has_friendly_breakable_cc_ignores_enemy_caster() {
    let me = Entity::from_raw(1);
    let enemy_caster = Entity::from_raw(2);
    let target = Entity::from_raw(3);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Warrior);
    snapshot
        .combatants
        .insert(enemy_caster, info(enemy_caster, 2, CharacterClass::Mage));
    snapshot
        .combatants
        .insert(target, info(target, 2, CharacterClass::Priest));

    // Enemy mage Polymorphed their own teammate. Not our problem — we can
    // damage that target without breaking *our* CC.
    snapshot.active_auras.insert(
        target,
        vec![aura_with(AuraType::Polymorph, Some(enemy_caster), 0.0)],
    );

    let ctx = snapshot.context_for(me);
    assert!(!ctx.has_friendly_breakable_cc(target));
}

#[test]
fn has_friendly_breakable_cc_ignores_high_threshold_auras() {
    let me = Entity::from_raw(1);
    let ally = Entity::from_raw(2);
    let enemy = Entity::from_raw(3);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Warrior);
    snapshot
        .combatants
        .insert(ally, info(ally, 1, CharacterClass::Mage));
    snapshot
        .combatants
        .insert(enemy, info(enemy, 2, CharacterClass::Rogue));

    // Frost Nova root: break_on_damage_threshold == 80.0, not 0.0. It absorbs
    // moderate damage before breaking, so attacking the target is fine.
    snapshot
        .active_auras
        .insert(enemy, vec![aura_with(AuraType::Root, Some(ally), 80.0)]);

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
    snapshot
        .combatants
        .insert(ally, info(ally, 1, CharacterClass::Warlock));
    snapshot
        .combatants
        .insert(enemy, info(enemy, 2, CharacterClass::Priest));

    // Warlock teammate has Corruption ticking. Polymorph would break next tick.
    snapshot.active_auras.insert(
        enemy,
        vec![aura_with(AuraType::DamageOverTime, Some(ally), -1.0)],
    );

    let ctx = snapshot.context_for(me);
    assert!(ctx.has_friendly_dots_on_target(enemy));
}

#[test]
fn has_friendly_dots_on_target_ignores_enemy_dot() {
    let me = Entity::from_raw(1);
    let enemy_warlock = Entity::from_raw(2);
    let teammate = Entity::from_raw(3);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Mage);
    snapshot.combatants.insert(
        enemy_warlock,
        info(enemy_warlock, 2, CharacterClass::Warlock),
    );
    snapshot
        .combatants
        .insert(teammate, info(teammate, 1, CharacterClass::Priest));

    // The DoT here is on a teammate, applied by an enemy — irrelevant to
    // whether *we* would break our own CC by Polymorphing the *target*.
    snapshot.active_auras.insert(
        teammate,
        vec![aura_with(
            AuraType::DamageOverTime,
            Some(enemy_warlock),
            -1.0,
        )],
    );

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
    let target = ctx
        .lowest_health_ally_below(0.9, f32::MAX, Vec3::ZERO)
        .expect("ally below 90%");
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
    let target = ctx
        .lowest_health_ally_below(0.9, f32::MAX, Vec3::ZERO)
        .expect("non-pet ally");
    assert_eq!(
        target.entity, ally,
        "pet must not be returned even though its HP is lowest"
    );
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
    let target = ctx
        .lowest_health_ally_below(0.9, 30.0, Vec3::ZERO)
        .expect("near ally");
    assert_eq!(target.entity, near);
}

#[test]
fn lowest_health_ally_below_returns_none_when_team_is_full_hp() {
    let me = Entity::from_raw(1);
    let ally = Entity::from_raw(2);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Priest);
    snapshot
        .combatants
        .insert(ally, info(ally, 1, CharacterClass::Warrior));

    let ctx = snapshot.context_for(me);
    // Threshold 0.9 — nobody is below it (self + ally are both at 100%).
    assert!(ctx
        .lowest_health_ally_below(0.9, f32::MAX, Vec3::ZERO)
        .is_none());
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
    snapshot
        .combatants
        .insert(target, info(target, 2, CharacterClass::Warrior));

    // Each of these aura types should make the target read as CC'd.
    for cc in [
        AuraType::Stun,
        AuraType::Fear,
        AuraType::Root,
        AuraType::Polymorph,
        AuraType::Incapacitate,
    ] {
        snapshot
            .active_auras
            .insert(target, vec![aura_with(cc, None, -1.0)]);
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
    snapshot
        .combatants
        .insert(target, info(target, 2, CharacterClass::Warrior));

    // DoT is a debuff but not CC.
    snapshot.active_auras.insert(
        target,
        vec![aura_with(AuraType::DamageOverTime, None, -1.0)],
    );
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
    snapshot
        .combatants
        .insert(target, info(target, 2, CharacterClass::Priest));
    // No DRTracker entry — target has never been CC'd.

    let ctx = snapshot.context_for(me);
    assert!(!ctx.is_dr_immune(target, DRCategory::Stuns));
}

#[test]
fn is_dr_immune_returns_true_after_three_stuns() {
    let me = Entity::from_raw(1);
    let target = Entity::from_raw(2);

    let mut snapshot = snapshot_for(me, 1, CharacterClass::Rogue);
    snapshot
        .combatants
        .insert(target, info(target, 2, CharacterClass::Priest));

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
    snapshot.combatants.insert(
        rogue,
        CombatantInfo {
            stealthed: true,
            target: Some(me),
            position: Vec3::new(3.0, 0.0, 0.0),
            ..info(rogue, 2, CharacterClass::Rogue)
        },
    );

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
    snapshot.combatants.insert(
        rogue,
        CombatantInfo {
            stealthed: true,
            target: Some(me),
            position: Vec3::new(3.0, 0.0, 0.0),
            ..info(rogue, 2, CharacterClass::Rogue)
        },
    );
    // I hold Shadow Sight — the stealthed Rogue is revealed.
    snapshot
        .active_auras
        .insert(me, vec![aura_with(AuraType::ShadowSight, None, -1.0)]);

    let ctx = snapshot.context_for(me);
    let threats = ctx.enemies_targeting(me);
    assert_eq!(
        threats.len(),
        1,
        "shadow sight reveals the stealthed threat"
    );
    assert_eq!(threats[0].entity, rogue);
}

#[test]
fn enemies_targeting_includes_rogue_holding_shadow_sight() {
    // The other arm of the can_see rule: an enemy that picked up Shadow
    // Sight is revealed even while stealthed.
    let (mut snapshot, me) = priest_snapshot();
    let rogue = Entity::from_raw(2);
    snapshot.combatants.insert(
        rogue,
        CombatantInfo {
            stealthed: true,
            target: Some(me),
            ..info(rogue, 2, CharacterClass::Rogue)
        },
    );
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
    snapshot.combatants.insert(
        pet,
        CombatantInfo {
            target: Some(me),
            position: Vec3::new(4.0, 0.0, 0.0),
            ..pet_info(pet, 2, CharacterClass::Warlock)
        },
    );

    let ctx = snapshot.context_for(me);
    let threats = ctx.enemies_targeting(me);
    assert_eq!(threats.len(), 1, "enemy pets count as threats");
    assert_eq!(threats[0].entity, pet);
}

#[test]
fn enemies_targeting_excludes_enemy_targeting_someone_else_and_dead() {
    let (mut snapshot, me) = priest_snapshot();
    let ally = Entity::from_raw(2);
    snapshot
        .combatants
        .insert(ally, info(ally, 1, CharacterClass::Warrior));
    // Enemy on my team's Warrior, not me.
    let warrior = Entity::from_raw(3);
    snapshot.combatants.insert(
        warrior,
        CombatantInfo {
            target: Some(ally),
            ..info(warrior, 2, CharacterClass::Warrior)
        },
    );
    // Dead enemy "targeting" me.
    let corpse = Entity::from_raw(4);
    snapshot.combatants.insert(
        corpse,
        CombatantInfo {
            target: Some(me),
            is_alive: false,
            current_health: 0.0,
            ..info(corpse, 2, CharacterClass::Rogue)
        },
    );

    let ctx = snapshot.context_for(me);
    assert!(ctx.enemies_targeting(me).is_empty());
}

#[test]
fn primary_attacker_picks_nearest_of_two() {
    let (mut snapshot, me) = priest_snapshot();
    let far = Entity::from_raw(2);
    snapshot.combatants.insert(
        far,
        CombatantInfo {
            target: Some(me),
            position: Vec3::new(20.0, 0.0, 0.0),
            ..info(far, 2, CharacterClass::Warrior)
        },
    );
    let near = Entity::from_raw(3);
    snapshot.combatants.insert(
        near,
        CombatantInfo {
            target: Some(me),
            position: Vec3::new(5.0, 0.0, 0.0),
            ..info(near, 2, CharacterClass::Rogue)
        },
    );

    let ctx = snapshot.context_for(me);
    let attacker = ctx.primary_attacker(me).expect("two attackers exist");
    assert_eq!(attacker.entity, near, "nearest attacker wins");
}

#[test]
fn primary_attacker_skips_dead_and_invisible() {
    let (mut snapshot, me) = priest_snapshot();
    // Nearest is dead.
    let dead = Entity::from_raw(2);
    snapshot.combatants.insert(
        dead,
        CombatantInfo {
            target: Some(me),
            position: Vec3::new(2.0, 0.0, 0.0),
            is_alive: false,
            current_health: 0.0,
            ..info(dead, 2, CharacterClass::Warrior)
        },
    );
    // Second-nearest is stealthed (invisible).
    let hidden = Entity::from_raw(3);
    snapshot.combatants.insert(
        hidden,
        CombatantInfo {
            target: Some(me),
            position: Vec3::new(4.0, 0.0, 0.0),
            stealthed: true,
            ..info(hidden, 2, CharacterClass::Rogue)
        },
    );
    // Farthest is the only live, visible attacker.
    let live = Entity::from_raw(4);
    snapshot.combatants.insert(
        live,
        CombatantInfo {
            target: Some(me),
            position: Vec3::new(15.0, 0.0, 0.0),
            ..info(live, 2, CharacterClass::Warrior)
        },
    );

    let ctx = snapshot.context_for(me);
    let attacker = ctx.primary_attacker(me).expect("one valid attacker");
    assert_eq!(attacker.entity, live);
}

#[test]
fn primary_attacker_none_when_unthreatened() {
    let (mut snapshot, me) = priest_snapshot();
    let enemy = Entity::from_raw(2);
    snapshot
        .combatants
        .insert(enemy, info(enemy, 2, CharacterClass::Mage)); // target: None

    let ctx = snapshot.context_for(me);
    assert!(ctx.primary_attacker(me).is_none());
}

#[test]
fn attacker_escape_window_returns_remaining_impair_duration() {
    let (mut snapshot, me) = priest_snapshot();
    let attacker = Entity::from_raw(2);
    snapshot.combatants.insert(
        attacker,
        CombatantInfo {
            target: Some(me),
            ..info(attacker, 2, CharacterClass::Warrior)
        },
    );

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
    snapshot
        .combatants
        .insert(attacker, info(attacker, 2, CharacterClass::Warrior));

    let mut short_stun = aura_with(AuraType::Stun, None, -1.0);
    short_stun.duration = 1.5;
    let mut long_root = aura_with(AuraType::Root, None, 80.0);
    long_root.duration = 6.0;
    snapshot
        .active_auras
        .insert(attacker, vec![short_stun, long_root]);

    let ctx = snapshot.context_for(me);
    assert_eq!(ctx.attacker_escape_window(attacker), Some(6.0));
}

#[test]
fn attacker_escape_window_none_for_fear_or_free_attacker() {
    let (mut snapshot, me) = priest_snapshot();
    let attacker = Entity::from_raw(2);
    snapshot
        .combatants
        .insert(attacker, info(attacker, 2, CharacterClass::Warrior));

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
    snapshot.combatants.insert(
        warrior,
        CombatantInfo {
            target: Some(me),
            position: Vec3::new(10.0, 0.0, 0.0),
            ..info(warrior, 2, CharacterClass::Warrior)
        },
    );

    let ctx = snapshot.context_for(me);
    assert!(ctx.is_closing(warrior, me));
}

#[test]
fn is_closing_false_for_stationary_caster_in_range() {
    let (mut snapshot, me) = priest_snapshot();
    // Mage (preferred_range 38.0) at 20 units targeting me: already inside
    // its preferred range, so pursuit holds position — not closing.
    let mage = Entity::from_raw(2);
    snapshot.combatants.insert(
        mage,
        CombatantInfo {
            target: Some(me),
            position: Vec3::new(20.0, 0.0, 0.0),
            ..info(mage, 2, CharacterClass::Mage)
        },
    );

    let ctx = snapshot.context_for(me);
    assert!(!ctx.is_closing(mage, me));
}

#[test]
fn is_closing_false_when_threat_targets_someone_else_or_is_in_melee() {
    let (mut snapshot, me) = priest_snapshot();
    let ally = Entity::from_raw(2);
    snapshot
        .combatants
        .insert(ally, info(ally, 1, CharacterClass::Warrior));

    // Distant melee whose kill target is my ally, not me.
    let off_target = Entity::from_raw(3);
    snapshot.combatants.insert(
        off_target,
        CombatantInfo {
            target: Some(ally),
            position: Vec3::new(10.0, 0.0, 0.0),
            ..info(off_target, 2, CharacterClass::Warrior)
        },
    );
    // Melee already on top of me (inside preferred_range 2.0): targeting me
    // but not "closing" — it is already there.
    let in_melee = Entity::from_raw(4);
    snapshot.combatants.insert(
        in_melee,
        CombatantInfo {
            target: Some(me),
            position: Vec3::new(1.5, 0.0, 0.0),
            ..info(in_melee, 2, CharacterClass::Rogue)
        },
    );

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
    snapshot.combatants.insert(
        felhunter,
        CombatantInfo {
            target: Some(me),
            position: Vec3::new(12.0, 0.0, 0.0),
            ..pet_info(felhunter, 2, CharacterClass::Warlock)
        },
    );

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
    snapshot.combatants.insert(
        near,
        CombatantInfo {
            position: Vec3::new(5.0, 0.0, 0.0),
            ..info(near, 2, CharacterClass::Warrior)
        },
    );
    // Enemy at distance 15 (outside radius 10).
    let far = Entity::from_raw(3);
    snapshot.combatants.insert(
        far,
        CombatantInfo {
            position: Vec3::new(15.0, 0.0, 0.0),
            ..info(far, 2, CharacterClass::Mage)
        },
    );

    let ctx = snapshot.context_for(me);
    let within: Vec<Entity> = ctx
        .visible_enemies_within(me, Vec3::ZERO, 10.0)
        .iter()
        .map(|c| c.entity)
        .collect();
    assert_eq!(
        within,
        vec![near],
        "only the enemy inside the radius is returned"
    );
}

#[test]
fn visible_enemies_within_respects_radius_boundary_and_team() {
    let (mut snapshot, me) = priest_snapshot();
    // Enemy exactly at the radius (10.0) — `<=` so it is included.
    let on_edge = Entity::from_raw(2);
    snapshot.combatants.insert(
        on_edge,
        CombatantInfo {
            position: Vec3::new(10.0, 0.0, 0.0),
            ..info(on_edge, 2, CharacterClass::Warrior)
        },
    );
    // Ally inside the radius — never a "threat", regardless of distance.
    let ally = Entity::from_raw(3);
    snapshot.combatants.insert(
        ally,
        CombatantInfo {
            position: Vec3::new(2.0, 0.0, 0.0),
            ..info(ally, 1, CharacterClass::Warrior)
        },
    );

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
    assert_eq!(
        ctx.movement_slow_multiplier(me),
        0.5,
        "one 50% slow halves speed"
    );
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
    snapshot
        .active_auras
        .insert(me, vec![slow_a, slow_b, unrelated]);

    let ctx = snapshot.context_for(me);
    // 0.5 * 0.7 = 0.35 (the DoT does not participate).
    assert!(
        (ctx.movement_slow_multiplier(me) - 0.35).abs() < 1e-6,
        "two slows multiply: got {}",
        ctx.movement_slow_multiplier(me)
    );
}

// ============================================================================
// team_hp_sums / team_hp_advantage — press-when-ahead signal
// ============================================================================

/// A full-HP `info` scaled to `hp_frac` of its max (0.0..=1.0).
fn injured(entity: Entity, team: u8, class: CharacterClass, hp_frac: f32) -> CombatantInfo {
    CombatantInfo {
        current_health: 100.0 * hp_frac,
        ..info(entity, team, class)
    }
}

/// Snapshot holding exactly the given members (self-entity is chosen per test
/// via `context_for`).
fn snapshot_with(members: &[CombatantInfo]) -> CombatSnapshot {
    let mut combatants = BTreeMap::new();
    for m in members {
        combatants.insert(m.entity, *m);
    }
    CombatSnapshot {
        ai_profile: Default::default(),
        bounds: Default::default(),
        combatants,
        active_auras: BTreeMap::new(),
        dr_trackers: BTreeMap::new(),
        ability_cooldowns: BTreeMap::new(),
        obstacles: Vec::new(),
    }
}

#[test]
fn team_hp_sums_excludes_pets_and_dead() {
    let warrior = info(Entity::from_raw(1), 0, CharacterClass::Warrior);
    let pet = pet_info(Entity::from_raw(2), 0, CharacterClass::Hunter);
    let mut dead = info(Entity::from_raw(3), 0, CharacterClass::Priest);
    dead.is_alive = false;
    let enemy = injured(Entity::from_raw(4), 1, CharacterClass::Mage, 0.5);

    let snap = snapshot_with(&[warrior, pet, dead, enemy]);
    let sums = team_hp_sums(&snap.combatants);

    // Team 0: only the full-HP warrior counts — the pet and the dead Priest
    // are both excluded, mirroring is_team_healthy's conventions.
    assert_eq!(sums.get(&0).copied(), Some(1.0));
    // Team 1: the half-HP mage contributes its fraction.
    assert_eq!(sums.get(&1).copied(), Some(0.5));
}

#[test]
fn team_hp_advantage_zero_for_equal_teams() {
    let a = info(Entity::from_raw(1), 0, CharacterClass::Warrior);
    let b = info(Entity::from_raw(2), 1, CharacterClass::Mage);
    let snap = snapshot_with(&[a, b]);
    assert_eq!(snap.context_for(a.entity).team_hp_advantage(), 0.0);
    assert_eq!(snap.context_for(b.entity).team_hp_advantage(), 0.0);
}

#[test]
fn team_hp_advantage_signs_and_symmetry() {
    // Team 0 full (1.0) vs team 1 at half (0.5): +0.5 for team 0, and the exact
    // negation for team 1.
    let a = info(Entity::from_raw(1), 0, CharacterClass::Warrior);
    let b = injured(Entity::from_raw(2), 1, CharacterClass::Mage, 0.5);
    let snap = snapshot_with(&[a, b]);
    assert_eq!(snap.context_for(a.entity).team_hp_advantage(), 0.5);
    assert_eq!(snap.context_for(b.entity).team_hp_advantage(), -0.5);
}

#[test]
fn team_hp_advantage_dead_member_drags_own_sum() {
    // A 2v1 where one of the pair is dead collapses the advantage to level:
    // team 0 (one full member) vs team 1 (one full member) → 0.0.
    let alive = info(Entity::from_raw(1), 0, CharacterClass::Warrior);
    let mut dead = info(Entity::from_raw(2), 0, CharacterClass::Priest);
    dead.is_alive = false;
    let enemy = info(Entity::from_raw(3), 1, CharacterClass::Mage);
    let snap = snapshot_with(&[alive, dead, enemy]);
    assert_eq!(snap.context_for(alive.entity).team_hp_advantage(), 0.0);
}

#[test]
fn team_hp_advantage_deterministic_across_rebuild() {
    let members = [
        injured(Entity::from_raw(1), 0, CharacterClass::Warrior, 0.7),
        injured(Entity::from_raw(2), 0, CharacterClass::Priest, 0.9),
        injured(Entity::from_raw(3), 1, CharacterClass::Mage, 0.4),
    ];
    let s1 = snapshot_with(&members);
    let s2 = snapshot_with(&members);
    assert_eq!(
        s1.context_for(members[0].entity).team_hp_advantage(),
        s2.context_for(members[0].entity).team_hp_advantage(),
        "rebuilding the snapshot must not change the differential (determinism)",
    );
}

#[test]
fn team_hp_advantage_zero_when_self_absent() {
    // Self missing from the snapshot → no team known → 0.0 (never presses).
    let a = info(Entity::from_raw(1), 0, CharacterClass::Warrior);
    let snap = snapshot_with(&[a]);
    assert_eq!(
        snap.context_for(Entity::from_raw(999)).team_hp_advantage(),
        0.0
    );
}

// ============================================================================
// Freezing Trap — who can free its victim (AS-125)
// ============================================================================

/// Who can lift a Freezing Trap off a teammate, asked of EVERY class and of
/// the Felhunter through the engine's own removal rules (`can_free_ally`), and
/// asserted as named sets. The Hunter's trap AI holds a trap whose victim an
/// enemy could free, so this table is what decides where traps go: a class
/// joining the non-pet row without anyone deciding it should would silently
/// stop every Hunter trapping its teammates.
///
/// The pet row is the AS-129 seam: today only the Felhunter reaches a pet, so
/// a trapped Felhunter is freed by nobody (it cannot act while trapped). Make
/// the healers' dispels reach pets and this row, not the Hunter AI, changes.
#[test]
fn who_can_free_a_freezing_trap() {
    use arenasim::states::play_match::ability_config::AbilityDefinitions;
    use arenasim::states::play_match::class_ai::can_free_ally;
    use arenasim::states::play_match::traps::freezing_trap_aura;

    let defs = AbilityDefinitions::default();
    let hunter = Entity::from_raw(99);
    let aura = freezing_trap_aura(hunter);
    let victim = info(Entity::from_raw(1), 2, CharacterClass::Warrior);
    let pet_victim = pet_info(Entity::from_raw(2), 2, CharacterClass::Warlock);

    let freers_of = |victim: &CombatantInfo| {
        let mut freers: Vec<String> = CharacterClass::all()
            .iter()
            .map(|class| info(Entity::from_raw(10), 2, *class))
            .chain(std::iter::once(pet_info(
                Entity::from_raw(11),
                2,
                CharacterClass::Warlock,
            )))
            .filter(|freer| can_free_ally(&defs, freer, victim, &aura))
            .map(|freer| match freer.pet_type {
                Some(pet) => pet.name().to_string(),
                None => freer.class.name().to_string(),
            })
            .collect();
        freers.sort();
        freers
    };

    assert_eq!(
        freers_of(&victim),
        vec!["Felhunter", "Paladin", "Priest"],
        "who frees a trapped teammate"
    );
    assert_eq!(
        freers_of(&pet_victim),
        vec!["Felhunter"],
        "who frees a trapped pet"
    );

    // Nobody frees themselves, and nobody frees an ENEMY.
    let priest = info(Entity::from_raw(1), 2, CharacterClass::Priest);
    assert!(!can_free_ally(&defs, &priest, &priest, &aura));
    let enemy_priest = info(Entity::from_raw(3), 1, CharacterClass::Priest);
    assert!(!can_free_ally(&defs, &enemy_priest, &victim, &aura));
}

/// The Hunter's "is this trap worth throwing" rule, over the team shapes the
/// AS-125 rulings name. A healer — the dispeller the trap is FOR — is always
/// worth one, including beside a Felhunter that could devour it (the intended
/// counter is left to play out). Anyone else is worth one only when no
/// teammate of its could free it — a trapped Felhunter included — however
/// hard the Hunter's team is pressing the teammate that would.
#[test]
fn a_trap_is_worth_throwing_where_nobody_frees_it() {
    use arenasim::states::play_match::ability_config::AbilityDefinitions;
    use arenasim::states::play_match::class_ai::hunter_dip::trap_victim_worth_it;

    let defs = AbilityDefinitions::default();
    let hunter = Entity::from_raw(0);
    let (a, b, felhunter) = (
        Entity::from_raw(1),
        Entity::from_raw(2),
        Entity::from_raw(3),
    );

    // Worth-it for `a`, `b` and (when fielded) the Felhunter, in that order.
    let worth = |team: &[(Entity, CharacterClass)], with_felhunter: bool| {
        let mut snap = snapshot_for(hunter, 1, CharacterClass::Hunter);
        for (e, class) in team {
            snap.combatants.insert(*e, info(*e, 2, *class));
        }
        if with_felhunter {
            snap.combatants
                .insert(felhunter, pet_info(felhunter, 2, CharacterClass::Warlock));
        }
        let ctx = snap.context_for(hunter);
        team.iter()
            .map(|(e, _)| *e)
            .chain(with_felhunter.then_some(felhunter))
            .map(|e| trap_victim_worth_it(&ctx, &defs, hunter, e))
            .collect::<Vec<_>>()
    };

    use CharacterClass::*;
    // The dispeller is the target; the DPS it would free is not — whoever the
    // Hunter's team is killing, since a focused Priest still dispels it.
    assert_eq!(worth(&[(a, Priest), (b, Warrior)], false), [true, false]);
    // Nobody on the team can free anybody: both are fair game.
    assert_eq!(worth(&[(a, Rogue), (b, Warrior)], false), [true, true]);
    // The Shaman frees nobody (Purge strips enemy buffs), so its DPS is fair.
    assert_eq!(worth(&[(a, Shaman), (b, Warrior)], false), [true, true]);
    // Warlock + healer: two dispellers. The healer stays the target; the
    // Warlock is freed by the Felhunter (and the Priest); the Felhunter itself
    // nobody can free.
    assert_eq!(
        worth(&[(a, Priest), (b, Warlock)], true),
        [true, false, true]
    );
    // Warlock + Rogue: the Felhunter frees both, so neither is worth a trap —
    // but a trapped Felhunter stays trapped.
    assert_eq!(
        worth(&[(a, Rogue), (b, Warlock)], true),
        [false, false, true]
    );
    // 1v1 Warlock: the Felhunter is fair game, its Warlock is not.
    assert_eq!(worth(&[(b, Warlock)], true), [false, true]);
}

/// A trap thrown into open ground can be sprung by an enemy the Hunter cannot
/// see, so while an enemy is hidden the question is asked of the visible
/// dispeller alone: at gates-open the enemy Rogue is stealthed and absent from
/// the Hunter's view, yet a lone visible Priest frees whoever walks into a
/// lane trap. With nothing hidden, the question does not arise.
#[test]
fn an_unseen_victim_is_answered_by_the_dispeller_in_view() {
    use arenasim::states::play_match::ability_config::AbilityDefinitions;
    use arenasim::states::play_match::class_ai::hunter_dip::unseen_victim_would_be_freed;

    let defs = AbilityDefinitions::default();
    let hunter = Entity::from_raw(0);
    let freed = |enemies: &[CombatantInfo]| {
        let mut snap = snapshot_for(hunter, 1, CharacterClass::Hunter);
        for e in enemies {
            snap.combatants.insert(e.entity, *e);
        }
        unseen_victim_would_be_freed(&snap.context_for(hunter), &defs, hunter, 1)
    };
    let e = |i: u32, class: CharacterClass| info(Entity::from_raw(i), 2, class);
    let hidden = |i: u32, class: CharacterClass| CombatantInfo {
        stealthed: true,
        ..e(i, class)
    };

    use CharacterClass::*;
    assert!(
        freed(&[hidden(1, Rogue), e(2, Priest)]),
        "a stealthed Rogue beside a visible Priest"
    );
    assert!(freed(&[hidden(1, Rogue), e(2, Paladin)]));
    assert!(freed(&[
        hidden(1, Rogue),
        e(2, Warlock),
        pet_info(Entity::from_raw(3), 2, Warlock)
    ]));
    assert!(!freed(&[hidden(1, Rogue), e(2, Warrior)]));
    assert!(!freed(&[hidden(1, Rogue), e(2, Shaman)]));
    // Nothing hidden: the expected victim is in view and answers for itself.
    assert!(!freed(&[e(1, Rogue), e(2, Priest)]));
    // A dead dispeller frees nobody.
    let dead_priest = CombatantInfo {
        is_alive: false,
        ..e(2, Priest)
    };
    assert!(!freed(&[hidden(1, Rogue), dead_priest]));
}

/// A lane trap is decided on the enemy that would SPRING it — the first inside
/// its radius once it has armed — not on whoever stands nearest the landing
/// when it is thrown. The throw here flies 15yd (0.75s) and arms 1.5s later.
#[test]
fn a_lane_trap_is_decided_on_whoever_reaches_it_after_it_arms() {
    use arenasim::states::play_match::class_ai::hunter_dip::predicted_trap_springer;

    let hunter = Entity::from_raw(0);
    let landing = Vec3::new(0.0, 0.0, 15.0);
    let at = |e: u32, class: CharacterClass, pos: Vec3, vel: Vec3, target: Option<Entity>| {
        CombatantInfo {
            position: pos,
            velocity: vel,
            target,
            ..info(Entity::from_raw(e), 2, class)
        }
    };
    let springer = |enemies: Vec<CombatantInfo>| {
        let mut snap = snapshot_for(hunter, 1, CharacterClass::Hunter);
        for e in enemies {
            snap.combatants.insert(e.entity, e);
        }
        let ctx = snap.context_for(hunter);
        predicted_trap_springer(&ctx, 1, Vec3::ZERO, landing)
    };
    let toward_hunter = Vec3::new(0.0, 0.0, -7.0);

    // The Warlock + Rogue case: a Felhunter stands nearest the landing, just
    // outside the radius, while the Rogue runs the lane at the Hunter. The
    // Rogue springs it.
    let felhunter = CombatantInfo {
        position: Vec3::new(8.0, 0.0, 15.0),
        ..pet_info(Entity::from_raw(1), 2, CharacterClass::Warlock)
    };
    let rogue = at(
        2,
        CharacterClass::Rogue,
        Vec3::new(0.0, 0.0, 40.0),
        toward_hunter,
        Some(hunter),
    );
    assert_eq!(springer(vec![felhunter, rogue]), Some(rogue.entity));
    // Alone, the Felhunter never reaches it: nobody is predicted.
    assert_eq!(springer(vec![felhunter]), None);

    // An enemy already inside the radius and planted springs it as it arms.
    let planted = at(3, CharacterClass::Warrior, landing, Vec3::ZERO, None);
    assert_eq!(springer(vec![planted]), Some(planted.entity));

    // An enemy that crosses the landing before it arms and runs on (chasing a
    // unit far behind the Hunter) is gone by the time it could spring.
    let through = at(
        4,
        CharacterClass::Warrior,
        Vec3::new(0.0, 0.0, 20.0),
        toward_hunter,
        Some(hunter),
    );
    let mut far_ally = info(Entity::from_raw(5), 1, CharacterClass::Priest);
    far_ally.position = Vec3::new(0.0, 0.0, -60.0);
    let mut snap = snapshot_for(hunter, 1, CharacterClass::Hunter);
    snap.combatants.insert(far_ally.entity, far_ally);
    snap.combatants.insert(
        through.entity,
        CombatantInfo {
            target: Some(far_ally.entity),
            ..through
        },
    );
    let ctx = snap.context_for(hunter);
    assert_eq!(predicted_trap_springer(&ctx, 1, Vec3::ZERO, landing), None);

    // A caster closing on the Hunter stops at its casting range, short of the
    // landing; a melee from the same spot runs through it.
    let priest = at(
        6,
        CharacterClass::Priest,
        Vec3::new(0.0, 0.0, 50.0),
        toward_hunter,
        Some(hunter),
    );
    assert_eq!(springer(vec![priest]), None);
    let warrior = at(
        7,
        CharacterClass::Warrior,
        Vec3::new(0.0, 0.0, 40.0),
        toward_hunter,
        Some(hunter),
    );
    assert_eq!(springer(vec![warrior]), Some(warrior.entity));
}

/// The trap breaks on any damage, so a lane trap is never decided on an enemy
/// the Hunter's own team is attacking: a teammate's target, or the Hunter's
/// kill target once it has a teammate to converge on it. Alone, the Hunter and
/// its pet hold fire on a trapped target, so the 1v1 peel on the kill target
/// stands.
#[test]
fn a_lane_trap_is_not_thrown_onto_the_enemy_the_team_is_hitting() {
    use arenasim::states::play_match::class_ai::hunter_dip::teammate_would_break_it;

    let hunter = Entity::from_raw(0);
    let (victim, other) = (Entity::from_raw(1), Entity::from_raw(2));
    let breaks = |ally_target: Option<Option<Entity>>, own_target: Option<Entity>| {
        let mut snap = snapshot_for(hunter, 1, CharacterClass::Hunter);
        snap.combatants
            .insert(victim, info(victim, 2, CharacterClass::Warrior));
        snap.combatants
            .insert(other, info(other, 2, CharacterClass::Priest));
        if let Some(target) = ally_target {
            let ally = Entity::from_raw(3);
            snap.combatants.insert(
                ally,
                CombatantInfo {
                    target,
                    ..info(ally, 1, CharacterClass::Priest)
                },
            );
        }
        let ctx = snap.context_for(hunter);
        teammate_would_break_it(&ctx, hunter, own_target, victim)
    };

    // 1v1 (no teammate): the peel on the Hunter's own kill target stands.
    assert!(!breaks(None, Some(victim)));
    // With a teammate, the kill target is off-limits...
    assert!(breaks(Some(None), Some(victim)));
    // ...and so is anyone a teammate is hitting, healers included...
    assert!(breaks(Some(Some(victim)), Some(other)));
    // ...but not an enemy nobody on the team is attacking.
    assert!(!breaks(Some(Some(other)), Some(other)));
}

/// The "trap the dispeller" ruling under pressure: with a melee on the Hunter,
/// the Freezing Trap goes on the enemy HEALER — when nothing would free it
/// and no DPS teammate of the Hunter's is killing it (a trapped enemy is one
/// every damage source holds fire on, so trapping a partner's target idles
/// that partner). Two targets do not hold it: a partner HEALER's — a healer's
/// target is wherever acquisition left it, usually the enemy healer, and
/// counting it would hold the trap in nearly every Hunter + healer team — and
/// the Hunter's own, which it leaves when it throws (target acquisition,
/// pinned in `tests/hunter_trap_trace.rs`).
#[test]
fn a_pressured_hunter_traps_the_healer_nobody_else_is_killing() {
    use arenasim::states::play_match::ability_config::AbilityDefinitions;
    use arenasim::states::play_match::class_ai::hunter_dip::{
        pressure_trap_healer, PRESSURE_TRAP_FREEABLE, PRESSURE_TRAP_INELIGIBLE,
        PRESSURE_TRAP_TEAM_ATTACKING,
    };

    let defs = AbilityDefinitions::default();
    let hunter = Entity::from_raw(0);
    let (healer, partner, felhunter, teammate) = (
        Entity::from_raw(1),
        Entity::from_raw(2),
        Entity::from_raw(3),
        Entity::from_raw(4),
    );
    use CharacterClass::*;
    let verdict = |healer_class: CharacterClass,
                   partner_class: CharacterClass,
                   with_felhunter: bool,
                   ally: Option<(CharacterClass, Option<Entity>)>,
                   own_target: Option<Entity>,
                   dotted: bool| {
        let mut snap = snapshot_for(hunter, 1, Hunter);
        snap.combatants.insert(
            hunter,
            CombatantInfo {
                target: own_target,
                ..snap.combatants[&hunter]
            },
        );
        snap.combatants
            .insert(healer, info(healer, 2, healer_class));
        snap.combatants
            .insert(partner, info(partner, 2, partner_class));
        if with_felhunter {
            snap.combatants
                .insert(felhunter, pet_info(felhunter, 2, Warlock));
        }
        if let Some((class, target)) = ally {
            snap.combatants.insert(
                teammate,
                CombatantInfo {
                    target,
                    ..info(teammate, 1, class)
                },
            );
        }
        if dotted {
            snap.active_auras.insert(
                healer,
                vec![Aura {
                    tick_interval: 3.0,
                    ..aura_with(AuraType::DamageOverTime, Some(hunter), 0.0)
                }],
            );
        }
        let ctx = snap.context_for(hunter);
        pressure_trap_healer(&ctx, &defs, hunter, 1)
    };

    // Rogue + Priest, the Hunter killing the Priest, and its own Priest's
    // target the Priest too: a partner healer does not hold it — trap the
    // Priest.
    assert_eq!(
        verdict(
            Priest,
            Rogue,
            false,
            Some((Priest, Some(healer))),
            Some(healer),
            false
        ),
        Some(Ok(healer))
    );
    // Its Warrior partner is killing the Priest: held.
    assert_eq!(
        verdict(
            Priest,
            Rogue,
            false,
            Some((Warrior, Some(healer))),
            Some(partner),
            false
        ),
        Some(Err(PRESSURE_TRAP_TEAM_ATTACKING))
    );
    // The partner is on the Rogue instead: trap the Priest.
    assert_eq!(
        verdict(
            Priest,
            Rogue,
            false,
            Some((Warrior, Some(partner))),
            Some(partner),
            false
        ),
        Some(Ok(healer))
    );
    // A Felhunter would devour it off the Priest: held.
    assert_eq!(
        verdict(Priest, Warlock, true, None, Some(partner), false),
        Some(Err(PRESSURE_TRAP_FREEABLE))
    );
    // A friendly DoT would break it on the first tick: held.
    assert_eq!(
        verdict(Priest, Rogue, false, None, Some(healer), true),
        Some(Err(PRESSURE_TRAP_INELIGIBLE))
    );
    // Paladin + Warrior, Shaman + Warrior: nothing frees either healer.
    assert_eq!(
        verdict(Paladin, Warrior, false, None, Some(partner), false),
        Some(Ok(healer))
    );
    assert_eq!(
        verdict(Shaman, Warrior, false, None, Some(partner), false),
        Some(Ok(healer))
    );
    // No healer at all: nothing to decide.
    assert_eq!(
        verdict(Mage, Warrior, false, None, Some(partner), false),
        None
    );
}

/// Where the pressure trap lands: on the healer's led position, only within
/// the trap's configured range, and only when the healer is the enemy that
/// would spring it — nobody else near the landing.
#[test]
fn a_pressure_trap_lands_only_where_it_catches_the_healer_cleanly() {
    use arenasim::states::play_match::abilities::AbilityType;
    use arenasim::states::play_match::ability_config::AbilityDefinitions;
    use arenasim::states::play_match::class_ai::hunter_dip::{
        pressure_trap_landing, PRESSURE_TRAP_NOT_CLEAN, PRESSURE_TRAP_OUT_OF_RANGE,
    };

    let defs = AbilityDefinitions::default();
    let range = defs.get_unchecked(&AbilityType::FreezingTrap).range;
    let hunter = Entity::from_raw(0);
    let (healer, rogue) = (Entity::from_raw(1), Entity::from_raw(2));
    let landing = |healer_at: Vec3, rogue_at: Vec3| {
        let mut snap = snapshot_for(hunter, 1, CharacterClass::Hunter);
        snap.combatants.insert(
            healer,
            CombatantInfo {
                position: healer_at,
                ..info(healer, 2, CharacterClass::Priest)
            },
        );
        snap.combatants.insert(
            rogue,
            CombatantInfo {
                position: rogue_at,
                target: Some(hunter),
                ..info(rogue, 2, CharacterClass::Rogue)
            },
        );
        pressure_trap_landing(&snap.context_for(hunter), &defs, 1, Vec3::ZERO, healer)
    };
    let on_hunter = Vec3::new(2.0, 0.0, 0.0);

    // A planted Priest 20yd off, the Rogue on the Hunter: dropped on it.
    let at = Vec3::new(0.0, 0.0, 20.0);
    assert_eq!(landing(at, on_hunter), Ok(at));
    // Beyond the configured range: held, not pulled in.
    assert_eq!(
        landing(Vec3::new(0.0, 0.0, range + 1.0), on_hunter),
        Err(PRESSURE_TRAP_OUT_OF_RANGE)
    );
    // The Rogue beside the Priest would spring it first: held.
    assert_eq!(
        landing(at, Vec3::new(2.0, 0.0, 20.0)),
        Err(PRESSURE_TRAP_NOT_CLEAN)
    );
}

/// Disengage folds in the trap setup the way the kite does: straight away
/// from the melee, bent toward the enemy healer only when the straight leap
/// would land beyond throw range of it, and never so far that it stops
/// carrying the Hunter away from the melee.
#[test]
fn disengage_bends_toward_the_healer_only_when_the_leap_would_lose_it() {
    use arenasim::states::play_match::class_ai::hunter_dip::trap_setup_disengage;

    let away = Vec3::new(-1.0, 0.0, 0.0);
    let bend = |healer: Vec3, trap_setup: f32| {
        trap_setup_disengage(away, Vec3::ZERO, 15.0, healer, 25.0, 6.0, trap_setup)
    };

    // The straight leap keeps the healer within 25yd: unchanged.
    assert_eq!(bend(Vec3::new(-10.0, 0.0, 10.0), 3.0), away);
    // It would carry the Hunter out of range: bent toward +Z, still away.
    let bent = bend(Vec3::new(10.0, 0.0, 30.0), 3.0);
    assert!(bent.x < 0.0 && bent.z > 0.1, "bent {bent:?}");
    assert!((bent.length() - 1.0).abs() < 1e-4);
    // Zero weight: unchanged.
    assert_eq!(bend(Vec3::new(10.0, 0.0, 30.0), 0.0), away);
    // The healer directly behind the melee: the bend cannot reverse the leap.
    let behind = bend(Vec3::new(40.0, 0.0, 0.0), 3.0);
    assert!(behind.dot(away) > 0.0, "still away, got {behind:?}");
}

/// Aimed Shot goes before a due Serpent Sting only when it can finish: no
/// enemy that could stop the 2.5s cast — an interrupt within its own reach, a
/// melee closing to melee range, or an enemy the Hunter cannot see — gets
/// there first.
#[test]
fn aimed_shot_waits_for_the_time_to_finish_it() {
    use arenasim::states::play_match::ability_config::AbilityDefinitions;
    use arenasim::states::play_match::class_ai::hunter::aimed_shot_has_time;

    let defs = AbilityDefinitions::default();
    let hunter = Entity::from_raw(0);
    let has_time = |enemies: Vec<CombatantInfo>, auras: Vec<(Entity, Aura)>| {
        let mut snap = snapshot_for(hunter, 1, CharacterClass::Hunter);
        for e in enemies {
            snap.combatants.insert(e.entity, e);
        }
        for (e, a) in auras {
            snap.active_auras.insert(e, vec![a]);
        }
        aimed_shot_has_time(
            &defs,
            hunter,
            Vec3::ZERO,
            None,
            &snap.context_for(hunter),
            0.0,
        )
    };
    let at = |i: u32, class: CharacterClass, z: f32, vz: f32| CombatantInfo {
        position: Vec3::new(0.0, 0.0, z),
        velocity: Vec3::new(0.0, 0.0, vz),
        ..info(Entity::from_raw(i), 2, class)
    };
    use CharacterClass::*;

    // Nobody who could stop it: a Priest and a Mage at 25yd.
    assert!(has_time(
        vec![at(1, Priest, 25.0, -7.0), at(2, Mage, 25.0, 0.0)],
        vec![]
    ));
    // A Warrior 30yd off, running in at 7yd/s: 3.9s to melee range.
    assert!(has_time(vec![at(1, Warrior, 30.0, -7.0)], vec![]));
    // The same Warrior 15yd off: 1.8s — it arrives mid-cast.
    assert!(!has_time(vec![at(1, Warrior, 15.0, -7.0)], vec![]));
    // A Rogue 15yd off walking away never arrives.
    assert!(has_time(vec![at(1, Rogue, 15.0, 7.0)], vec![]));
    // A Felhunter inside Spell Lock's 30yd needs no time at all...
    let felhunter = CombatantInfo {
        position: Vec3::new(0.0, 0.0, 25.0),
        ..pet_info(Entity::from_raw(3), 2, Warlock)
    };
    assert!(!has_time(vec![felhunter], vec![]));
    // ...unless it is held in hard CC past the cast.
    let trapped = Aura {
        duration: 6.0,
        ..aura_with(AuraType::Incapacitate, Some(hunter), 0.0)
    };
    assert!(has_time(vec![felhunter], vec![(felhunter.entity, trapped)]));
    // A Rogue the Hunter cannot see counts as one that could stop it: the
    // Priest beside it is harmless, but the opener waits.
    let stealthed_rogue = CombatantInfo {
        stealthed: true,
        ..at(4, Rogue, 60.0, 0.0)
    };
    assert!(!has_time(
        vec![at(1, Priest, 25.0, 0.0), stealthed_rogue],
        vec![]
    ));
}

/// AS-179 — the opener puts Aimed Shot ahead of a due Serpent Sting only when
/// the sting's GCD would cost it the window: it can finish if begun now and
/// could not if begun after the sting. Against an enemy that cannot stop the
/// cast at all (a lone Priest) there is time for both, so the sting goes
/// first — which is what the Hunter vs Priest 1v1 had lost.
#[test]
fn aimed_shot_goes_before_the_sting_only_when_the_sting_would_cost_it_the_window() {
    use arenasim::states::play_match::ability_config::AbilityDefinitions;
    use arenasim::states::play_match::class_ai::hunter::aimed_shot_before_sting;

    let defs = AbilityDefinitions::default();
    let hunter = Entity::from_raw(0);
    let before_sting = |enemy: CombatantInfo| {
        let mut snap = snapshot_for(hunter, 1, CharacterClass::Hunter);
        snap.combatants.insert(enemy.entity, enemy);
        aimed_shot_before_sting(&defs, hunter, Vec3::ZERO, None, &snap.context_for(hunter))
    };
    let at = |class: CharacterClass, z: f32, vz: f32| CombatantInfo {
        position: Vec3::new(0.0, 0.0, z),
        velocity: Vec3::new(0.0, 0.0, vz),
        ..info(Entity::from_raw(1), 2, class)
    };
    use CharacterClass::*;

    // A Priest walking in cannot stop the cast: time for both, sting first.
    assert!(!before_sting(at(Priest, 25.0, -7.0)));
    // A Warrior 30yd off at 7yd/s reaches the dead zone in 3.1s: after a cast
    // begun now (2.5s), before one begun after the sting's GCD (4.0s).
    assert!(before_sting(at(Warrior, 30.0, -7.0)));
    // 45yd off it arrives in 5.3s: time for both, sting first.
    assert!(!before_sting(at(Warrior, 45.0, -7.0)));
    // 15yd off it arrives mid-cast either way: no Aimed Shot opener.
    assert!(!before_sting(at(Warrior, 15.0, -7.0)));
}

/// AS-179 — the Hunter's own Freezing Trap is not broken by its own shot.
///
/// A Rogue 20yd off runs at the Hunter through a trap landing 10yd out, which
/// arms in 1s: it is inside the trap's radius from 0.83s. An Aimed Shot that
/// lands 3s from now would land on it frozen; an Arcane Shot landing in 0.5s
/// lands before the trap has armed and cannot break it. A trap off the Rogue's
/// line, or a Rogue running the other way, is never sprung by it.
#[test]
fn a_shot_that_would_land_after_the_hunters_own_trap_catches_its_target_is_held() {
    use arenasim::states::play_match::class_ai::hunter_dip::{own_trap_catches_first, LiveTrap};

    let hunter = Entity::from_raw(0);
    let rogue = Entity::from_raw(1);
    let catches = |vz: f32, trap: LiveTrap, lands_after: f32| {
        let mut snap = snapshot_for(hunter, 1, CharacterClass::Hunter);
        snap.combatants.insert(
            rogue,
            CombatantInfo {
                position: Vec3::new(0.0, 0.0, 20.0),
                velocity: Vec3::new(0.0, 0.0, vz),
                target: Some(hunter),
                ..info(rogue, 2, CharacterClass::Rogue)
            },
        );
        own_trap_catches_first(&snap.context_for(hunter), &[trap], rogue, lands_after)
    };
    let in_lane = LiveTrap {
        position: Vec3::new(0.0, 0.0, 10.0),
        armed_in: 1.0,
    };

    assert!(catches(-6.0, in_lane, 3.0), "Aimed Shot would break it");
    assert!(!catches(-6.0, in_lane, 0.5), "lands before the trap arms");
    // Standing on a trap that has not armed yet: a shot landing first is safe.
    let underfoot = LiveTrap {
        position: Vec3::new(0.0, 0.0, 18.0),
        ..in_lane
    };
    assert!(!catches(-6.0, underfoot, 0.5), "lands before the trap arms");
    assert!(catches(-6.0, underfoot, 3.0), "lands after it springs");
    assert!(!catches(6.0, in_lane, 3.0), "the Rogue runs away from it");
    let off_line = LiveTrap {
        position: Vec3::new(20.0, 0.0, 10.0),
        ..in_lane
    };
    assert!(
        !catches(-6.0, off_line, 3.0),
        "the trap is off the Rogue's line"
    );
}
