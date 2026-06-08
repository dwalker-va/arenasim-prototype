//! Unit tests for `CombatSnapshot`.
//!
//! `from_queries`/`build` is exercised end-to-end by every headless match
//! (`tests/headless_tests.rs`) plus the registration audit. These tests cover
//! the query-independent behavior: `context_for` round-tripping, and
//! `reflect_instant_cc` updating both `active_auras` and `dr_trackers` so
//! later combatants in the same frame's dispatch loop observe the CC.

use std::collections::BTreeMap;

use bevy::ecs::system::SystemState;
use bevy::prelude::*;

use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::class_ai::combat_snapshot::CombatSnapshot;
use arenasim::states::play_match::class_ai::CombatantInfo;
use arenasim::states::play_match::{
    ActiveAuras, Aura, AuraType, CastingState, ChannelingState, Combatant, DRCategory, DRTracker,
    Pet,
};

fn target_info(entity: Entity, team: u8, class: CharacterClass) -> CombatantInfo {
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
        pet: None,
    }
}

fn make_aura(effect_type: AuraType, ability_name: &str) -> Aura {
    Aura {
        effect_type,
        duration: 4.0,
        magnitude: 1.0,
        break_on_damage_threshold: -1.0,
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

fn empty_snapshot_with(caster: Entity, target: Entity) -> CombatSnapshot {
    let mut combatants = BTreeMap::new();
    combatants.insert(caster, target_info(caster, 1, CharacterClass::Mage));
    combatants.insert(target, target_info(target, 2, CharacterClass::Warrior));
    CombatSnapshot {
        combatants,
        active_auras: BTreeMap::new(),
        dr_trackers: BTreeMap::new(),
        ability_cooldowns: BTreeMap::new(),
    }
}

#[test]
fn context_for_returns_view_into_snapshot() {
    let caster = Entity::from_raw(1);
    let target = Entity::from_raw(2);
    let snapshot = empty_snapshot_with(caster, target);

    let ctx = snapshot.context_for(caster);
    assert_eq!(ctx.self_entity, caster);
    assert!(ctx.alive_enemies().iter().any(|info| info.entity == target));
    assert!(ctx.alive_allies().iter().any(|info| info.entity == caster));
}

#[test]
fn reflect_instant_cc_makes_target_visible_as_ccd() {
    let caster = Entity::from_raw(1);
    let target = Entity::from_raw(2);
    let mut snapshot = empty_snapshot_with(caster, target);

    let stun = make_aura(AuraType::Stun, "Cheap Shot");
    snapshot.reflect_instant_cc(target, &stun);

    // The reflected aura now shows up in the snapshot's active_auras...
    let auras = snapshot.active_auras.get(&target).expect("aura was inserted");
    assert!(auras.iter().any(|a| a.effect_type == AuraType::Stun));

    // ...and a CombatContext built from this snapshot reports the target as CC'd.
    let ctx = snapshot.context_for(caster);
    assert!(ctx.is_ccd(target));
}

#[test]
fn reflect_instant_cc_advances_existing_dr_tracker() {
    // The snapshot's DR tracker is built from the live `DRTracker` component
    // attached to each combatant in `decide_abilities`. The reflection helper
    // updates an existing tracker but does not create one. (When a target has
    // never been CC'd, no DRTracker component exists yet — the real aura
    // application path creates it. The reflection path is meant only to keep
    // existing trackers in sync within the same frame.)
    let caster = Entity::from_raw(1);
    let target = Entity::from_raw(2);
    let mut snapshot = empty_snapshot_with(caster, target);

    let mut tracker = DRTracker::default();
    let multiplier_first = tracker.apply(DRCategory::Stuns);
    snapshot.dr_trackers.insert(target, tracker);

    let stun = make_aura(AuraType::Stun, "Hammer of Justice");
    snapshot.reflect_instant_cc(target, &stun);

    // The reflected aura's duration should be DR-scaled by the SECOND application,
    // since one was already applied above. DR multipliers are monotonically
    // decreasing, so applying again must produce <= multiplier_first.
    let aura = snapshot.active_auras.get(&target).and_then(|a| a.first()).expect("aura inserted");
    let multiplier_used = aura.duration / 4.0;
    assert!(multiplier_used <= multiplier_first);
}

#[test]
fn reflect_instant_cc_skips_target_with_damage_immunity() {
    let caster = Entity::from_raw(1);
    let target = Entity::from_raw(2);
    let mut snapshot = empty_snapshot_with(caster, target);

    // Pre-existing Divine Shield on the target.
    snapshot.active_auras.insert(target, vec![make_aura(AuraType::DamageImmunity, "Divine Shield")]);

    let stun = make_aura(AuraType::Stun, "Cheap Shot");
    snapshot.reflect_instant_cc(target, &stun);

    // The reflection helper bails when the target is damage-immune; the only
    // aura on them is still Divine Shield, no Stun was added.
    let auras = snapshot.active_auras.get(&target).expect("auras present");
    assert_eq!(auras.len(), 1);
    assert_eq!(auras[0].effect_type, AuraType::DamageImmunity);
    assert!(snapshot.dr_trackers.get(&target).is_none());
}

#[test]
fn reflect_instant_cc_respects_existing_dr_immunity() {
    let caster = Entity::from_raw(1);
    let target = Entity::from_raw(2);
    let mut snapshot = empty_snapshot_with(caster, target);

    // Pre-stage a DR tracker that's already immune to Stuns.
    let mut tracker = DRTracker::default();
    // Apply Stuns three times to drive the category to immunity.
    tracker.apply(DRCategory::Stuns);
    tracker.apply(DRCategory::Stuns);
    tracker.apply(DRCategory::Stuns);
    assert!(tracker.is_immune(DRCategory::Stuns));
    snapshot.dr_trackers.insert(target, tracker);

    let stun = make_aura(AuraType::Stun, "Cheap Shot");
    snapshot.reflect_instant_cc(target, &stun);

    // DR-immune targets reject the reflection — no aura was added.
    assert!(snapshot.active_auras.get(&target).map_or(true, |a| a.is_empty()));
}

// ---------------------------------------------------------------------------
// `build` visibility of casting/channeling combatants (U4.1)
//
// Before this fix, `build` only constructed `combatants` entries from the
// query filtered `Without<CastingState>`/`Without<ChannelingState>` — every
// mid-cast combatant was invisible to all class AI (healers could not see a
// casting ally's HP; nobody could see a casting enemy's target).
// ---------------------------------------------------------------------------

/// Run `CombatSnapshot::build` against a real Bevy `World`, with the same
/// query shapes `decide_abilities` holds.
fn build_snapshot_from_world(world: &mut World) -> CombatSnapshot {
    let mut state: SystemState<(
        Query<
            (Entity, &'static mut Combatant, &'static Transform, Option<&'static mut ActiveAuras>),
            (Without<CastingState>, Without<ChannelingState>),
        >,
        Query<
            (Entity, &'static Combatant, &'static Transform, Option<&'static ActiveAuras>),
            With<CastingState>,
        >,
        Query<
            (Entity, &'static Combatant, &'static Transform, Option<&'static ActiveAuras>),
            (With<ChannelingState>, Without<CastingState>),
        >,
        Query<(Entity, &'static DRTracker)>,
        Query<&'static Pet>,
    )> = SystemState::new(world);
    let (aura_q, casting_q, channeling_q, dr_q, pet_q) = state.get_mut(world);
    CombatSnapshot::build(&aura_q, &casting_q, &channeling_q, &dr_q, &pet_q)
}

fn spawn_combatant(world: &mut World, team: u8, slot: u8, class: CharacterClass, pos: Vec3) -> Entity {
    world
        .spawn((
            Combatant::new(team, slot, class),
            Transform::from_translation(pos),
        ))
        .id()
}

#[test]
fn build_includes_mid_cast_enemy_with_readable_target() {
    let mut world = World::new();
    let me = spawn_combatant(&mut world, 1, 0, CharacterClass::Priest, Vec3::ZERO);
    let enemy = spawn_combatant(&mut world, 2, 0, CharacterClass::Mage, Vec3::new(20.0, 0.0, 0.0));

    // The enemy Mage is mid-Frostbolt at me.
    world.entity_mut(enemy).get_mut::<Combatant>().unwrap().target = Some(me);
    world.entity_mut(enemy).insert(CastingState::new(AbilityType::Frostbolt, me, 2.5));

    let snapshot = build_snapshot_from_world(&mut world);

    let info = snapshot
        .combatants
        .get(&enemy)
        .expect("mid-cast enemy must be present in the snapshot");
    assert_eq!(info.target, Some(me), "casting enemy's target must be readable");
    assert_eq!(info.team, 2);
    assert_eq!(info.position, Vec3::new(20.0, 0.0, 0.0));
    assert!(info.is_alive);

    // And it shows up through the CombatContext enemy view.
    let ctx = snapshot.context_for(me);
    assert!(
        ctx.alive_enemies().iter().any(|c| c.entity == enemy),
        "casting enemy must appear in alive_enemies()"
    );
}

#[test]
fn build_exposes_casting_ally_hp() {
    let mut world = World::new();
    let healer = spawn_combatant(&mut world, 1, 0, CharacterClass::Priest, Vec3::ZERO);
    let ally = spawn_combatant(&mut world, 1, 1, CharacterClass::Mage, Vec3::new(5.0, 0.0, 0.0));
    let _enemy = spawn_combatant(&mut world, 2, 0, CharacterClass::Warrior, Vec3::new(30.0, 0.0, 0.0));

    // The ally is mid-cast at 40% HP — the healer must see that HP.
    {
        let mut c = world.entity_mut(ally);
        let mut combatant = c.get_mut::<Combatant>().unwrap();
        combatant.current_health = combatant.max_health * 0.4;
    }
    world.entity_mut(ally).insert(CastingState::new(AbilityType::Frostbolt, healer, 2.5));

    let snapshot = build_snapshot_from_world(&mut world);

    let info = snapshot.combatants.get(&ally).expect("casting ally must be in snapshot");
    assert!((info.health_pct() - 0.4).abs() < 1e-6, "casting ally HP must be visible");

    let ctx = snapshot.context_for(healer);
    let lowest = ctx.lowest_health_ally().expect("healer sees at least one ally");
    assert_eq!(
        lowest.entity, ally,
        "the casting ally at 40% must be the lowest-health ally the healer sees"
    );
}

#[test]
fn build_includes_channeling_combatant_with_auras() {
    let mut world = World::new();
    let me = spawn_combatant(&mut world, 1, 0, CharacterClass::Warrior, Vec3::ZERO);
    let enemy = spawn_combatant(&mut world, 2, 0, CharacterClass::Warlock, Vec3::new(15.0, 0.0, 0.0));

    world.entity_mut(enemy).get_mut::<Combatant>().unwrap().target = Some(me);
    world.entity_mut(enemy).insert((
        ChannelingState {
            ability: AbilityType::DrainLife,
            duration_remaining: 4.0,
            time_until_next_tick: 1.0,
            tick_interval: 1.0,
            target: me,
            interrupted: false,
            interrupted_display_time: 0.0,
            ticks_applied: 0,
        },
        ActiveAuras { auras: vec![make_aura(AuraType::MovementSpeedSlow, "Concussive Shot")] },
    ));

    let snapshot = build_snapshot_from_world(&mut world);

    let info = snapshot
        .combatants
        .get(&enemy)
        .expect("channeling enemy must be present in the snapshot");
    assert_eq!(info.target, Some(me));

    // Aura harvesting for channeling entities (pre-existing behavior) still works.
    let auras = snapshot.active_auras.get(&enemy).expect("channeling enemy auras harvested");
    assert!(auras.iter().any(|a| a.effect_type == AuraType::MovementSpeedSlow));
}
