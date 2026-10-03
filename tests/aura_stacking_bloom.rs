//! Stacking, refreshing and blooming auras, driven through the real systems.
//!
//! The pure halves (`AuraStacks::added`, `Aura::tick_amount`,
//! `refresh_stacking_aura`) are unit-tested beside them. What only the systems
//! can show is that the PATHS use them: `apply_pending_auras` refreshes a
//! recast instead of refusing it, `process_hot_ticks` heals per stack and
//! notices an aura's last frame, `process_dispels` blooms what it takes, and
//! `process_blooms` is where every bloom lands. Mark of the Wild rides the same
//! paths as the first compound BUFF.

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::auras::{apply_pending_auras, process_hot_ticks, update_auras};
use arenasim::states::play_match::combat_core::process_casting;
use arenasim::states::play_match::combat_core::{
    MARK_OF_THE_WILD_ARMOR, MARK_OF_THE_WILD_RESISTANCE, MARK_OF_THE_WILD_RESISTED_SCHOOLS,
};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, CastingState, Combatant,
    CompoundDebuff, DispelPending, DispelScope, GameRng,
};
use arenasim::states::play_match::effects::{process_blooms, process_dispels};
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::{AbilityDefinitions, AbilityType};
use arenasim::CharacterClass;

/// A world holding what the aura systems read, with `dt` seconds on the clock.
fn world(dt: f32) -> World {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(GameRng::from_seed(7));
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(dt));
    world.insert_resource(time);
    world
}

fn druid_and_ally(world: &mut World) -> (Entity, Entity) {
    let druid = world
        .spawn((
            Combatant::new(1, 1, CharacterClass::Druid),
            Transform::default(),
        ))
        .id();
    let mut hurt = Combatant::new(1, 0, CharacterClass::Warrior);
    hurt.current_health = 100.0;
    let ally = world.spawn((hurt, Transform::default())).id();
    (druid, ally)
}

/// The aura `ability` lands, as `process_casting` builds it (scaled by the
/// caster's spell power).
fn cast(ability: AbilityType, target: Entity, caster: Entity, spell_power: f32) -> AuraPending {
    let defs = AbilityDefinitions::default();
    AuraPending::from_ability_scaled(target, caster, defs.get_unchecked(&ability), spell_power)
        .expect("the ability applies an aura")
}

fn land(world: &mut World, pending: AuraPending) {
    world.spawn(pending);
    world.run_system_once(apply_pending_auras).unwrap();
}

fn auras_named(world: &World, entity: Entity, name: &str) -> Vec<Aura> {
    world
        .entity(entity)
        .get::<ActiveAuras>()
        .map(|a| {
            a.auras
                .iter()
                .filter(|aura| aura.ability_name == name)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

fn health(world: &World, entity: Entity) -> f32 {
    world
        .entity(entity)
        .get::<Combatant>()
        .unwrap()
        .current_health
}

fn log_has(world: &World, needle: &str) -> bool {
    world
        .resource::<CombatLog>()
        .entries
        .iter()
        .any(|e| e.message.contains(needle))
}

#[test]
fn a_recast_stacks_to_its_max_and_refreshes_instead_of_doubling() {
    let mut world = world(0.0);
    let (druid, ally) = druid_and_ally(&mut world);

    for expected in [1u8, 2, 3, 3] {
        let mut pending = cast(AbilityType::Lifebloom, ally, druid, 100.0);
        // Age the live aura first, so the refresh has something to restore.
        if let Some(mut auras) = world.entity_mut(ally).get_mut::<ActiveAuras>() {
            for aura in auras.auras.iter_mut() {
                aura.duration = 2.0;
            }
        }
        pending.aura.caster = Some(druid);
        land(&mut world, pending);

        let lifebloom = auras_named(&world, ally, "Lifebloom");
        assert_eq!(
            lifebloom.len(),
            1,
            "a recast must refresh, never add a copy"
        );
        assert_eq!(lifebloom[0].stack_count(), expected);
        assert_eq!(
            lifebloom[0].duration, 7.0,
            "the refresh restores the duration"
        );
    }
    assert!(log_has(&world, "Lifebloom refreshed (3 stacks)"));
}

#[test]
fn a_refresh_only_aura_renews_without_deepening() {
    let mut world = world(0.0);
    let (druid, ally) = druid_and_ally(&mut world);
    land(
        &mut world,
        cast(AbilityType::Rejuvenation, ally, druid, 100.0),
    );
    land(
        &mut world,
        cast(AbilityType::Rejuvenation, ally, druid, 100.0),
    );
    let rejuv = auras_named(&world, ally, "Rejuvenation");
    assert_eq!(rejuv.len(), 1);
    assert_eq!(rejuv[0].stack_count(), 1);
}

#[test]
fn a_stacking_heal_coexists_with_a_same_type_aura_from_another_source() {
    let mut world = world(0.0);
    let (druid, ally) = druid_and_ally(&mut world);
    land(
        &mut world,
        cast(AbilityType::Rejuvenation, ally, druid, 100.0),
    );
    land(&mut world, cast(AbilityType::Lifebloom, ally, druid, 100.0));
    // A one-per-type heal over time (the Healing Stream pulse's shape) lands
    // beside them: neither Druid heal holds the type's slot.
    land(
        &mut world,
        AuraPending {
            target: ally,
            aura: Aura {
                effect_type: AuraType::HealingOverTime,
                duration: 3.0,
                magnitude: 8.0,
                tick_interval: 2.0,
                caster: Some(druid),
                ability_name: "Healing Stream".to_string(),
                ..Default::default()
            },
        },
    );
    let kinds = world.entity(ally).get::<ActiveAuras>().unwrap().auras.len();
    assert_eq!(kinds, 3, "all three heals over time must be up at once");
}

#[test]
fn each_tick_heals_per_stack() {
    let mut world = world(1.0);
    let (druid, ally) = druid_and_ally(&mut world);
    let mut aura = cast(AbilityType::Lifebloom, ally, druid, 100.0).aura;
    aura.stacks = aura
        .stacks
        .map(|s| arenasim::states::play_match::AuraStacks { count: 3, ..s });
    aura.time_until_next_tick = 0.5;
    let per_stack = aura.magnitude;
    world
        .entity_mut(ally)
        .insert(ActiveAuras { auras: vec![aura] });

    let before = health(&world, ally);
    world.run_system_once(process_hot_ticks).unwrap();
    assert_eq!(health(&world, ally) - before, per_stack * 3.0);
}

#[test]
fn a_bloom_lands_when_the_aura_runs_out_scaled_by_its_stacks() {
    let mut world = world(1.0);
    let (druid, ally) = druid_and_ally(&mut world);
    let mut aura = cast(AbilityType::Lifebloom, ally, druid, 100.0).aura;
    aura.stacks = aura
        .stacks
        .map(|s| arenasim::states::play_match::AuraStacks { count: 3, ..s });
    aura.duration = 0.5; // its last frame
    aura.time_until_next_tick = 5.0; // no ordinary tick this frame
    let per_stack = aura.bloom.expect("Lifebloom blooms");
    world
        .entity_mut(ally)
        .insert(ActiveAuras { auras: vec![aura] });

    let before = health(&world, ally);
    world.run_system_once(process_hot_ticks).unwrap();
    world.run_system_once(update_auras).unwrap();
    world.run_system_once(process_blooms).unwrap();

    let gained = health(&world, ally) - before;
    // The final tick fires too (the HoT cadence's last tick at expiry).
    let final_tick = cast(AbilityType::Lifebloom, ally, druid, 100.0)
        .aura
        .magnitude
        * 3.0;
    assert_eq!(gained, per_stack * 3.0 + final_tick);
    assert!(auras_named(&world, ally, "Lifebloom").is_empty());
    assert!(log_has(&world, "Lifebloom blooms on"));
    assert_eq!(
        world.entity(druid).get::<Combatant>().unwrap().healing_done,
        per_stack * 3.0,
        "the bloom is credited to its caster"
    );
}

#[test]
fn a_bloom_is_dampened_like_every_heal() {
    let mut world = world(1.0);
    world.resource_mut::<ArenaDampening>().reduction = 0.5;
    let (druid, ally) = druid_and_ally(&mut world);
    let mut aura = cast(AbilityType::Lifebloom, ally, druid, 100.0).aura;
    aura.duration = 0.5;
    aura.time_until_next_tick = 5.0;
    let per_stack = aura.bloom.unwrap();
    let tick = aura.magnitude;
    world
        .entity_mut(ally)
        .insert(ActiveAuras { auras: vec![aura] });

    let before = health(&world, ally);
    world.run_system_once(process_hot_ticks).unwrap();
    world.run_system_once(process_blooms).unwrap();
    assert_eq!(health(&world, ally) - before, (per_stack + tick) * 0.5);
}

#[test]
fn a_bloom_is_cut_by_healing_reduction() {
    // Mortal Strike cuts all healing received: the bloom, and the ordinary
    // tick that fires on the same last frame.
    let mut world = world(1.0);
    let (druid, ally) = druid_and_ally(&mut world);
    let mut aura = cast(AbilityType::Lifebloom, ally, druid, 100.0).aura;
    aura.duration = 0.5;
    aura.time_until_next_tick = 5.0;
    let per_stack = aura.bloom.unwrap();
    let tick = aura.magnitude;
    let mortal_wound = Aura {
        effect_type: AuraType::HealingReduction,
        duration: 10.0,
        magnitude: 0.5,
        ability_name: "Mortal Strike".to_string(),
        ..Default::default()
    };
    world.entity_mut(ally).insert(ActiveAuras {
        auras: vec![aura, mortal_wound],
    });

    let before = health(&world, ally);
    world.run_system_once(process_hot_ticks).unwrap();
    world.run_system_once(process_blooms).unwrap();
    assert_eq!(health(&world, ally) - before, (tick + per_stack) * 0.5);
}

#[test]
fn a_landed_swiftmend_consumes_the_targets_rejuvenation() {
    let mut world = world(1.0 / 60.0);
    world.insert_resource(AbilityDefinitions::default());
    world.insert_resource(ActiveMapGeometry {
        bounds: Default::default(),
        volumes: Vec::new(),
        cover_anchors: Vec::new(),
    });
    let (druid, ally) = druid_and_ally(&mut world);
    let rejuv = cast(AbilityType::Rejuvenation, ally, druid, 100.0).aura;
    world
        .entity_mut(ally)
        .insert(ActiveAuras { auras: vec![rejuv] });
    world.entity_mut(druid).insert(CastingState {
        ability: AbilityType::Swiftmend,
        time_remaining: 0.001,
        target: Some(ally),
        interrupted: false,
        interrupted_display_time: 0.0,
    });

    let before = health(&world, ally);
    world.run_system_once(process_casting).unwrap();

    assert!(
        auras_named(&world, ally, "Rejuvenation").is_empty(),
        "Swiftmend must eat the Rejuvenation it heals with"
    );
    assert!(health(&world, ally) > before, "and heal");
    assert!(log_has(&world, "Swiftmend consumes Rejuvenation on"));
}

/// Mark of the Wild's resistance riders are `SpellResistanceBuff`s. They are
/// part of the Mark, not the one-per-type resistance slot, so a Paladin's
/// Shadow Resistance Aura lands beside them whichever of the two arrives first.
#[test]
fn mark_of_the_wild_and_shadow_resistance_aura_land_in_either_order() {
    for mark_first in [true, false] {
        let mut world = world(0.0);
        let (druid, ally) = druid_and_ally(&mut world);
        let paladin = world
            .spawn((
                Combatant::new(1, 2, CharacterClass::Paladin),
                Transform::default(),
            ))
            .id();
        let defs = AbilityDefinitions::default();
        let aura_pending = || {
            AuraPending::from_ability(
                ally,
                paladin,
                defs.get_unchecked(&AbilityType::ShadowResistanceAura),
            )
            .unwrap()
        };
        if mark_first {
            land(
                &mut world,
                cast(AbilityType::MarkOfTheWild, ally, druid, 0.0),
            );
            land(&mut world, aura_pending());
        } else {
            land(&mut world, aura_pending());
            land(
                &mut world,
                cast(AbilityType::MarkOfTheWild, ally, druid, 0.0),
            );
        }
        let order = if mark_first {
            "Mark first"
        } else {
            "aura first"
        };
        assert_eq!(
            auras_named(&world, ally, "Shadow Resistance Aura").len(),
            1,
            "{order}: the Paladin's aura must land"
        );
        assert!(
            !mark_of_the_wild_effects(&world, ally).is_empty(),
            "{order}: the Mark must land"
        );
    }
}

#[test]
fn a_bloom_lands_when_the_aura_is_purged() {
    let mut world = world(0.0);
    let (druid, ally) = druid_and_ally(&mut world);
    let mut aura = cast(AbilityType::Lifebloom, ally, druid, 100.0).aura;
    aura.stacks = aura
        .stacks
        .map(|s| arenasim::states::play_match::AuraStacks { count: 2, ..s });
    let per_stack = aura.bloom.unwrap();
    world
        .entity_mut(ally)
        .insert(ActiveAuras { auras: vec![aura] });
    let shaman = world
        .spawn(Combatant::new(2, 0, CharacterClass::Shaman))
        .id();
    world.spawn(DispelPending {
        target: ally,
        dispeller: shaman,
        log_prefix: "[PURGE]",
        caster_class: CharacterClass::Shaman,
        heal_on_success: None,
        scope: DispelScope::Purge(AuraType::HealingOverTime),
    });

    let before = health(&world, ally);
    world.run_system_once(process_dispels).unwrap();
    world.run_system_once(process_blooms).unwrap();
    assert!(auras_named(&world, ally, "Lifebloom").is_empty());
    assert_eq!(health(&world, ally) - before, per_stack * 2.0);
}

#[test]
fn a_refresh_does_not_bloom() {
    let mut world = world(0.0);
    let (druid, ally) = druid_and_ally(&mut world);
    land(&mut world, cast(AbilityType::Lifebloom, ally, druid, 100.0));
    land(&mut world, cast(AbilityType::Lifebloom, ally, druid, 100.0));
    world.run_system_once(process_blooms).unwrap();
    assert!(!log_has(&world, "blooms on"));
    assert_eq!(health(&world, ally), 100.0);
}

fn mark_of_the_wild_effects(world: &World, target: Entity) -> Vec<Aura> {
    world
        .entity(target)
        .get::<ActiveAuras>()
        .map(|a| {
            a.auras
                .iter()
                .filter(|aura| aura.compound == Some(CompoundDebuff::MarkOfTheWild))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn mark_of_the_wild_lands_whole() {
    let mut world = world(0.0);
    let (druid, ally) = druid_and_ally(&mut world);
    let base_max = world.entity(ally).get::<Combatant>().unwrap().max_health;
    land(
        &mut world,
        cast(AbilityType::MarkOfTheWild, ally, druid, 0.0),
    );

    let effects = mark_of_the_wild_effects(&world, ally);
    let armor: Vec<&Aura> = effects
        .iter()
        .filter(|a| a.effect_type == AuraType::ArmorIncrease)
        .collect();
    assert_eq!(armor.len(), 1);
    assert_eq!(armor[0].magnitude, MARK_OF_THE_WILD_ARMOR);
    let mut resisted: Vec<_> = effects
        .iter()
        .filter(|a| a.effect_type == AuraType::SpellResistanceBuff)
        .map(|a| {
            assert_eq!(a.magnitude, MARK_OF_THE_WILD_RESISTANCE);
            format!("{:?}", a.spell_school.unwrap())
        })
        .collect();
    resisted.sort();
    let mut expected: Vec<_> = MARK_OF_THE_WILD_RESISTED_SCHOOLS
        .iter()
        .map(|s| format!("{s:?}"))
        .collect();
    expected.sort();
    assert_eq!(resisted, expected);
    assert!(
        effects.iter().all(|a| a.duration == 600.0),
        "every rider shares the face's lifetime"
    );
    let bonus = world.entity(ally).get::<Combatant>().unwrap().max_health - base_max;
    assert!(bonus > 0.0, "the face raises maximum health");
}

#[test]
fn a_purge_takes_all_of_mark_of_the_wild_and_its_health() {
    let mut world = world(0.0);
    let (druid, ally) = druid_and_ally(&mut world);
    let base_max = world.entity(ally).get::<Combatant>().unwrap().max_health;
    land(
        &mut world,
        cast(AbilityType::MarkOfTheWild, ally, druid, 0.0),
    );
    let shaman = world
        .spawn(Combatant::new(2, 0, CharacterClass::Shaman))
        .id();
    world.spawn(DispelPending {
        target: ally,
        dispeller: shaman,
        log_prefix: "[PURGE]",
        caster_class: CharacterClass::Shaman,
        heal_on_success: None,
        scope: DispelScope::Purge(AuraType::MaxHealthIncrease),
    });
    world.run_system_once(process_dispels).unwrap();

    assert!(
        mark_of_the_wild_effects(&world, ally).is_empty(),
        "a purge left part of Mark of the Wild behind"
    );
    assert_eq!(
        world.entity(ally).get::<Combatant>().unwrap().max_health,
        base_max,
        "the purged buff must take its health with it"
    );
}

#[test]
fn a_rider_is_never_a_purge_candidate() {
    let mut world = world(0.0);
    let (druid, ally) = druid_and_ally(&mut world);
    land(
        &mut world,
        cast(AbilityType::MarkOfTheWild, ally, druid, 0.0),
    );
    let before = mark_of_the_wild_effects(&world, ally).len();
    let shaman = world
        .spawn(Combatant::new(2, 0, CharacterClass::Shaman))
        .id();
    // Pinned to a RIDER's type: nothing else on the target is a candidate.
    world.spawn(DispelPending {
        target: ally,
        dispeller: shaman,
        log_prefix: "[PURGE]",
        caster_class: CharacterClass::Shaman,
        heal_on_success: None,
        scope: DispelScope::Purge(AuraType::SpellResistanceBuff),
    });
    world.run_system_once(process_dispels).unwrap();
    assert_eq!(mark_of_the_wild_effects(&world, ally).len(), before);
}

// ----------------------------------------------------------------------------
// Two Druids: a heal over time is the CASTER's, a buff is the TARGET's
// (`StackScope`).
// ----------------------------------------------------------------------------

fn second_druid(world: &mut World) -> Entity {
    world
        .spawn((
            Combatant::new(1, 2, CharacterClass::Druid),
            Transform::default(),
        ))
        .id()
}

fn lifebloom_of(world: &World, ally: Entity, caster: Entity) -> Vec<Aura> {
    auras_named(world, ally, "Lifebloom")
        .into_iter()
        .filter(|a| a.caster == Some(caster))
        .collect()
}

#[test]
fn two_druids_each_keep_their_own_lifebloom_stack() {
    let mut world = world(0.0);
    let (druid_a, ally) = druid_and_ally(&mut world);
    let druid_b = second_druid(&mut world);

    land(&mut world, cast(AbilityType::Lifebloom, ally, druid_a, 100.0));
    land(&mut world, cast(AbilityType::Lifebloom, ally, druid_a, 100.0));
    land(&mut world, cast(AbilityType::Lifebloom, ally, druid_b, 100.0));
    assert_eq!(
        auras_named(&world, ally, "Lifebloom").len(),
        2,
        "the second Druid's Lifebloom lands beside the first's"
    );
    let a = lifebloom_of(&world, ally, druid_a);
    let b = lifebloom_of(&world, ally, druid_b);
    assert_eq!((a.len(), b.len()), (1, 1));
    assert_eq!(a[0].stack_count(), 2, "B's cast must not deepen A's stack");
    assert_eq!(b[0].stack_count(), 1);

    // One Druid's recast still refreshes and deepens its OWN stack only.
    if let Some(mut auras) = world.entity_mut(ally).get_mut::<ActiveAuras>() {
        for aura in auras.auras.iter_mut() {
            aura.duration = 2.0;
        }
    }
    land(&mut world, cast(AbilityType::Lifebloom, ally, druid_a, 100.0));
    let a = lifebloom_of(&world, ally, druid_a);
    let b = lifebloom_of(&world, ally, druid_b);
    assert_eq!(auras_named(&world, ally, "Lifebloom").len(), 2);
    assert_eq!((a[0].stack_count(), a[0].duration), (3, 7.0));
    assert_eq!(
        (b[0].stack_count(), b[0].duration),
        (1, 2.0),
        "A's refresh leaves B's stack alone"
    );
}

/// Both Druids' first Lifebloom in ONE frame, on an ally with no auras yet:
/// the same-frame path (`new_auras_map`, the per-frame applied set) keeps them
/// apart too.
#[test]
fn two_druids_lifeblooms_in_one_frame_land_as_two_stacks() {
    let mut world = world(0.0);
    let (druid_a, ally) = druid_and_ally(&mut world);
    let druid_b = second_druid(&mut world);
    world.spawn(cast(AbilityType::Lifebloom, ally, druid_a, 100.0));
    world.spawn(cast(AbilityType::Lifebloom, ally, druid_b, 100.0));
    world.run_system_once(apply_pending_auras).unwrap();
    assert_eq!(lifebloom_of(&world, ally, druid_a).len(), 1);
    assert_eq!(lifebloom_of(&world, ally, druid_b).len(), 1);
}

#[test]
fn each_druids_lifebloom_blooms_separately() {
    let mut world = world(1.0);
    let (druid_a, ally) = druid_and_ally(&mut world);
    let druid_b = second_druid(&mut world);
    let ending = |caster: Entity, stacks: u8, spell_power: f32| {
        let mut aura = cast(AbilityType::Lifebloom, ally, caster, spell_power).aura;
        aura.stacks = aura
            .stacks
            .map(|s| arenasim::states::play_match::AuraStacks { count: stacks, ..s });
        aura.duration = 0.5; // its last frame
        aura.time_until_next_tick = 5.0; // no ordinary tick this frame
        aura
    };
    let a = ending(druid_a, 3, 100.0);
    let b = ending(druid_b, 1, 40.0);
    let (bloom_a, bloom_b) = (a.bloom_heal().unwrap(), b.bloom_heal().unwrap());
    world
        .entity_mut(ally)
        .insert(ActiveAuras { auras: vec![a, b] });

    world.run_system_once(process_hot_ticks).unwrap();
    world.run_system_once(update_auras).unwrap();
    world.run_system_once(process_blooms).unwrap();

    let blooms = world
        .resource::<CombatLog>()
        .entries
        .iter()
        .filter(|e| e.message.contains("Lifebloom blooms on"))
        .count();
    assert_eq!(blooms, 2, "each Druid's stack blooms on its own");
    let healing = |e: Entity| world.entity(e).get::<Combatant>().unwrap().healing_done;
    assert_eq!(healing(druid_a), bloom_a, "A's bloom: its 3 stacks, its spell power");
    assert_eq!(healing(druid_b), bloom_b, "B's bloom: its 1 stack, its spell power");
}

#[test]
fn two_druids_marks_of_the_wild_do_not_stack() {
    let mut world = world(0.0);
    let (druid_a, ally) = druid_and_ally(&mut world);
    let druid_b = second_druid(&mut world);
    let base_max = world.entity(ally).get::<Combatant>().unwrap().max_health;
    land(
        &mut world,
        cast(AbilityType::MarkOfTheWild, ally, druid_a, 0.0),
    );
    let one_mark = mark_of_the_wild_effects(&world, ally).len();
    land(
        &mut world,
        cast(AbilityType::MarkOfTheWild, ally, druid_b, 0.0),
    );
    assert_eq!(
        mark_of_the_wild_effects(&world, ally).len(),
        one_mark,
        "a second Druid's Mark refreshes the first, never lands beside it"
    );
    let def = AbilityDefinitions::default();
    let bonus = def
        .get_unchecked(&AbilityType::MarkOfTheWild)
        .applies_aura
        .as_ref()
        .unwrap()
        .magnitude;
    assert_eq!(
        world.entity(ally).get::<Combatant>().unwrap().max_health,
        base_max + bonus,
        "the health bonus is granted once"
    );
    assert!(log_has(&world, "Mark of the Wild refreshed"));
}

/// Two Druids' Rejuvenations on one ally, and a purge pinned to one of them
/// (`DispelScope::PurgeSource` with its `owner`): it takes that one, whatever
/// the random pick among candidates would have rolled.
#[test]
fn a_purge_takes_the_druids_rejuvenation_it_chose() {
    for seed in 0..16u64 {
        for victim in [0usize, 1] {
            let mut world = world(0.0);
            world.insert_resource(GameRng::from_seed(seed));
            let (druid_a, ally) = druid_and_ally(&mut world);
            let druid_b = second_druid(&mut world);
            let casters = [druid_a, druid_b];
            land(
                &mut world,
                cast(AbilityType::Rejuvenation, ally, druid_a, 100.0),
            );
            land(
                &mut world,
                cast(AbilityType::Rejuvenation, ally, druid_b, 100.0),
            );
            assert_eq!(auras_named(&world, ally, "Rejuvenation").len(), 2);
            let shaman = world
                .spawn(Combatant::new(2, 0, CharacterClass::Shaman))
                .id();
            world.spawn(DispelPending {
                target: ally,
                dispeller: shaman,
                log_prefix: "[PURGE]",
                caster_class: CharacterClass::Shaman,
                heal_on_success: None,
                scope: DispelScope::PurgeSource {
                    effect: AuraType::HealingOverTime,
                    source: "Rejuvenation".to_string(),
                    owner: Some(casters[victim]),
                },
            });
            world.run_system_once(process_dispels).unwrap();
            let left: Vec<Option<Entity>> = auras_named(&world, ally, "Rejuvenation")
                .iter()
                .map(|a| a.caster)
                .collect();
            assert_eq!(
                left,
                vec![Some(casters[1 - victim])],
                "seed {seed}: the purge must take the chosen Druid's Rejuvenation"
            );
        }
    }
}

/// Swiftmend eats its caster's OWN Rejuvenation when two Druids have one up —
/// here the second in the aura list, so a first-match would take the wrong one.
#[test]
fn swiftmend_eats_its_casters_own_rejuvenation() {
    let mut world = world(1.0 / 60.0);
    world.insert_resource(AbilityDefinitions::default());
    world.insert_resource(ActiveMapGeometry {
        bounds: Default::default(),
        volumes: Vec::new(),
        cover_anchors: Vec::new(),
    });
    let (druid_a, ally) = druid_and_ally(&mut world);
    let druid_b = second_druid(&mut world);
    let rejuv_a = cast(AbilityType::Rejuvenation, ally, druid_a, 100.0).aura;
    let rejuv_b = cast(AbilityType::Rejuvenation, ally, druid_b, 100.0).aura;
    world.entity_mut(ally).insert(ActiveAuras {
        auras: vec![rejuv_a, rejuv_b],
    });
    world.entity_mut(druid_b).insert(CastingState {
        ability: AbilityType::Swiftmend,
        time_remaining: 0.001,
        target: Some(ally),
        interrupted: false,
        interrupted_display_time: 0.0,
    });
    world.run_system_once(process_casting).unwrap();

    let left: Vec<Option<Entity>> = auras_named(&world, ally, "Rejuvenation")
        .iter()
        .map(|a| a.caster)
        .collect();
    assert_eq!(left, vec![Some(druid_a)], "B's Swiftmend eats B's Rejuvenation");
}
