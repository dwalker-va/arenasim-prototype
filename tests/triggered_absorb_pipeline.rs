//! AS-429 — how a triggered absorb (Aegis of the Last Stand) composes with the
//! damage pipeline (AS-408).
//!
//! The shield is an ordinary `Absorb` aura, so the funnel
//! (`combat_core::apply_damage_with_absorb`) soaks with it at its absorb step:
//! after the damage-taken modifiers and a transfer's share, before the
//! killing-blow save and health. And it is raised by
//! `fire_triggered_absorbs`, which runs after `process_damage_follow_ups`, so
//! it judges the bearer after a save has had its say. Each claim is a pair:
//! the case, and its control with the one condition removed.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::combat_core::{
    apply_damage_with_absorb, process_damage_follow_ups, DamageFollowUp, DamageFollowUps, Hit,
    TransferReceiver,
};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraType, Combatant,
};
use arenasim::states::play_match::equipment::{load_item_definitions, ItemId};
use arenasim::states::play_match::talent_mechanics::{TalentMechanic, TalentMechanics};
use arenasim::states::play_match::triggered_absorbs::{
    fire_triggered_absorbs, AbsorbSource, ShieldFrom, TriggeredAbsorbConfig, TriggeredAbsorbSlot,
};

const AEGIS: &str = "Aegis of the Last Stand";

/// Aegis as `items.ron` ships it.
fn aegis() -> TriggeredAbsorbConfig {
    load_item_definitions()
        .expect("items.ron loads")
        .get_unchecked(&ItemId::AegisOfTheLastStand)
        .triggered_absorb
        .expect("Aegis carries a triggered absorb")
        .absorb
}

/// The shield Aegis raises, `points` strong.
fn shield(points: f32) -> Aura {
    aegis().shield_aura(AEGIS, ShieldFrom::Item(ItemId::AegisOfTheLastStand), points)
}

/// A Rogue with `health` of 400 and no armor or resistance, so a hit's number
/// is the pipeline's alone.
fn rogue(health: f32) -> Combatant {
    let mut c = Combatant::new(1, 0, CharacterClass::Rogue);
    c.max_health = 400.0;
    c.current_health = health;
    c.armor = 0.0;
    c.fire_resistance = 0.0;
    c.frost_resistance = 0.0;
    c.shadow_resistance = 0.0;
    c.arcane_resistance = 0.0;
    c.nature_resistance = 0.0;
    c.holy_resistance = 0.0;
    c
}

fn with_cheat_death(mut c: Combatant) -> Combatant {
    c.talent_mechanics = TalentMechanics::of(&[TalentMechanic::CheatDeath {
        health: 0.10,
        reduction: 0.90,
        duration: 3.0,
    }]);
    c
}

fn wearing_aegis(mut c: Combatant) -> Combatant {
    let slot = TriggeredAbsorbSlot::new(
        AEGIS,
        AbsorbSource::Item(ItemId::AegisOfTheLastStand),
        aegis(),
        c.damage_taken,
    );
    c.triggered_absorbs.push(slot);
    c
}

fn aura(kind: AuraType, magnitude: f32, caster: Option<Entity>) -> Aura {
    Aura {
        effect_type: kind,
        duration: 30.0,
        magnitude,
        break_on_damage_threshold: -1.0,
        ability_name: format!("{kind:?}"),
        caster,
        ..Default::default()
    }
}

fn the_shield(auras: &ActiveAuras) -> Option<f32> {
    auras
        .auras
        .iter()
        .find(|a| a.effect_type == AuraType::Absorb && a.ability_name == AEGIS)
        .map(|a| a.magnitude)
}

/// Land a direct `amount` Fire hit on `target`: (to health, absorbed,
/// follow-ups).
fn strike(
    amount: f32,
    target: &mut Combatant,
    auras: &mut ActiveAuras,
    attacker: Entity,
    victim: Entity,
) -> (f32, f32, Vec<DamageFollowUp>) {
    let mut follow_ups = DamageFollowUps::default();
    let (to_health, absorbed) = apply_damage_with_absorb(
        Hit::new(
            amount,
            SpellSchool::Fire,
            "Probe",
            Some(attacker),
            None,
            victim,
        ),
        target,
        Some(auras),
        &mut follow_ups,
        &mut CombatLog::default(),
    );
    (to_health, absorbed, follow_ups.as_slice().to_vec())
}

fn entities() -> (Entity, Entity, Entity) {
    let mut world = World::new();
    (
        world.spawn_empty().id(),
        world.spawn_empty().id(),
        world.spawn_empty().id(),
    )
}

/// The shield soaks the hit as the damage-taken modifiers left it: a 100 hit
/// halved by a reduction costs it 50, not 100. Control: the same hit without
/// the reduction costs it all 84.
#[test]
fn the_shield_soaks_the_hit_after_the_damage_taken_modifiers() {
    let (attacker, victim, _) = entities();
    let mut target = rogue(100.0);
    let mut auras = ActiveAuras {
        auras: vec![
            shield(84.0),
            aura(AuraType::DamageTakenReduction, 0.5, None),
        ],
    };
    let (to_health, absorbed, _) = strike(100.0, &mut target, &mut auras, attacker, victim);
    assert_eq!((to_health, absorbed), (0.0, 50.0));
    assert_eq!(the_shield(&auras), Some(34.0));

    let mut target = rogue(100.0);
    let mut auras = ActiveAuras {
        auras: vec![shield(84.0)],
    };
    let (to_health, absorbed, _) = strike(100.0, &mut target, &mut auras, attacker, victim);
    assert_eq!((to_health, absorbed), (16.0, 84.0));
    assert_eq!(the_shield(&auras), None, "spent, and removed");
}

/// A transfer takes its share before the shield soaks: of a 100 hit, half
/// leaves for the transfer's caster and the shield soaks the half that stays.
/// Control: without the transfer the shield soaks 84 of it.
#[test]
fn the_shield_soaks_only_what_a_transfer_leaves_the_bearer() {
    let (attacker, victim, paladin) = entities();
    let mut target = rogue(300.0);
    let mut auras = ActiveAuras {
        auras: vec![
            shield(84.0),
            aura(AuraType::DamageTransfer, 0.5, Some(paladin)),
        ],
    };
    let (to_health, absorbed, follow_ups) =
        strike(100.0, &mut target, &mut auras, attacker, victim);
    assert_eq!((to_health, absorbed), (0.0, 50.0));
    assert!(matches!(
        follow_ups.as_slice(),
        [DamageFollowUp::Transfer { receiver: TransferReceiver::Unit(r), amount, .. }]
            if *r == paladin && *amount == 50.0
    ));

    let mut target = rogue(300.0);
    let mut auras = ActiveAuras {
        auras: vec![shield(84.0)],
    };
    let (to_health, absorbed, _) = strike(100.0, &mut target, &mut auras, attacker, victim);
    assert_eq!((to_health, absorbed), (16.0, 84.0));
}

/// The shield soaks before the killing-blow save is judged: a blow that would
/// kill the bearer, soaked below its health, spends no Cheat Death. Control:
/// the same blow with no shield is a killing blow, and Cheat Death takes it.
#[test]
fn a_blow_the_shield_soaks_below_lethal_spends_no_save() {
    let (attacker, victim, _) = entities();
    let mut target = with_cheat_death(rogue(120.0));
    let mut auras = ActiveAuras {
        auras: vec![shield(80.0)],
    };
    let (to_health, absorbed, follow_ups) =
        strike(150.0, &mut target, &mut auras, attacker, victim);
    assert_eq!((to_health, absorbed), (70.0, 80.0));
    assert_eq!(target.current_health, 50.0);
    assert!(!target.killing_blow_save_spent);
    assert!(follow_ups.is_empty());

    let mut target = with_cheat_death(rogue(120.0));
    let mut auras = ActiveAuras::default();
    strike(150.0, &mut target, &mut auras, attacker, victim);
    assert!(
        target.killing_blow_save_spent,
        "the control's blow was lethal"
    );
    assert_eq!(target.current_health, 40.0, "left at 10% of 400");
}

/// One lethal blow from above the line, on a Rogue wearing Aegis: what is
/// left of it after the frame's follow-ups and the trigger have run. Returns
/// (health, the shield, the slot spent).
fn lethal_blow_from_above_the_line(cheat_death: bool) -> (f32, Option<f32>, bool) {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    let mut bearer = wearing_aegis(rogue(240.0));
    if cheat_death {
        bearer = with_cheat_death(bearer);
    }
    let attacker = world.spawn_empty().id();
    let victim = world.spawn((bearer, ActiveAuras::default())).id();

    // The blow, through the funnel, as a damage system lands it.
    let mut follow_ups = DamageFollowUps::default();
    {
        let mut units = world.query::<(&mut Combatant, &mut ActiveAuras)>();
        let (mut target, mut auras) = units.get_mut(&mut world, victim).unwrap();
        apply_damage_with_absorb(
            Hit::new(
                300.0,
                SpellSchool::Fire,
                "Probe",
                Some(attacker),
                None,
                victim,
            ),
            &mut target,
            Some(&mut auras),
            &mut follow_ups,
            &mut CombatLog::default(),
        );
    }
    {
        let mut commands = world.commands();
        follow_ups.spawn(&mut commands);
    }
    world.flush();
    // The end of the frame, in the schedule's order.
    world
        .run_system_once(process_damage_follow_ups)
        .expect("ran");
    world.run_system_once(fire_triggered_absorbs).expect("ran");

    let bearer = world.get::<Combatant>(victim).unwrap();
    let health = bearer.current_health;
    let spent = bearer.triggered_absorbs[0].spent();
    let shield = world.get::<ActiveAuras>(victim).and_then(the_shield);
    (health, shield, spent)
}

/// Cheat Death and Aegis on one blow: the save leaves the Rogue alive below
/// the line, so the shield rises on the frame of the blow, 20% of 400.
/// Control: without the save the blow kills, and a corpse raises nothing.
#[test]
fn a_save_that_leaves_the_bearer_below_the_line_raises_the_shield_that_frame() {
    assert_eq!(
        lethal_blow_from_above_the_line(true),
        (40.0, Some(80.0), true)
    );
    assert_eq!(lethal_blow_from_above_the_line(false), (0.0, None, false));
}

/// A bearer with no aura list yet still gets its shield: the trigger inserts
/// one. Control: one that already has a list keeps what is on it.
#[test]
fn the_shield_lands_with_or_without_an_aura_list() {
    for listed in [false, true] {
        let mut world = World::new();
        world.insert_resource(CombatLog::default());
        world.insert_resource(ArenaDampening::default());
        let bearer = wearing_aegis(rogue(100.0));
        let e = if listed {
            world
                .spawn((
                    bearer,
                    ActiveAuras {
                        auras: vec![aura(AuraType::DamageTakenReduction, 0.1, None)],
                    },
                ))
                .id()
        } else {
            world.spawn(bearer).id()
        };
        world.run_system_once(fire_triggered_absorbs).expect("ran");
        let auras = world.get::<ActiveAuras>(e).expect("an aura list");
        assert_eq!(the_shield(auras), Some(80.0), "listed: {listed}");
        assert_eq!(auras.auras.len(), if listed { 2 } else { 1 });
    }
}

/// A bearer with no aura list, given one by another system's command that
/// has not been applied when the trigger runs (no sync point between the
/// two): the shield joins that list rather than replacing it. Control: the
/// other system's aura alone is what is left when the bearer is above the
/// line.
#[test]
fn a_shield_landing_beside_another_new_aura_list_keeps_both() {
    fn give_a_list(mut commands: Commands, bearers: Query<Entity, With<Combatant>>) {
        for e in bearers.iter() {
            commands.entity(e).insert(ActiveAuras {
                auras: vec![aura(AuraType::DamageTakenReduction, 0.1, None)],
            });
        }
    }
    for below in [true, false] {
        let mut world = World::new();
        world.insert_resource(CombatLog::default());
        world.insert_resource(ArenaDampening::default());
        let e = world
            .spawn(wearing_aegis(rogue(if below { 100.0 } else { 300.0 })))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems((give_a_list, fire_triggered_absorbs).chain_ignore_deferred());
        schedule.run(&mut world);
        let auras = world.get::<ActiveAuras>(e).expect("an aura list");
        let kinds: Vec<AuraType> = auras.auras.iter().map(|a| a.effect_type).collect();
        if below {
            assert_eq!(
                kinds,
                vec![AuraType::DamageTakenReduction, AuraType::Absorb]
            );
        } else {
            assert_eq!(kinds, vec![AuraType::DamageTakenReduction]);
        }
    }
}

/// At full dampening the trigger still fires, and is spent, but lands no
/// empty shield for the visual to draw. Control: at half dampening it lands a
/// half shield.
#[test]
fn a_fully_dampened_shield_is_spent_but_lands_nothing() {
    for (reduction, want) in [(1.0, None), (0.5, Some(40.0))] {
        let mut world = World::new();
        world.insert_resource(CombatLog::default());
        world.insert_resource(ArenaDampening {
            reduction,
            ..Default::default()
        });
        let e = world
            .spawn((wearing_aegis(rogue(100.0)), ActiveAuras::default()))
            .id();
        world.run_system_once(fire_triggered_absorbs).expect("ran");
        assert!(world.get::<Combatant>(e).unwrap().triggered_absorbs[0].spent());
        let auras = world.get::<ActiveAuras>(e).unwrap();
        assert_eq!(the_shield(auras), want, "reduction {reduction}");
        assert_eq!(auras.auras.len(), usize::from(want.is_some()));
    }
}
