//! AS-454 — a Drain Life tick's log line names the damage that landed.
//!
//! The tick used to be logged as it was chosen, with the pre-pipeline number:
//! a tick a damage-taken cut halved, or a shield soaked, still printed the full
//! 10. It is now logged where it lands, after the pipeline, the way the DoT,
//! cast and projectile lines are, with its killing blow known at that point
//! (so the kill is credited on the tick itself, with no back-patch).

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::{CombatLog, StructuredEventData};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::combat_core::process_channeling;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraType, ChannelingState, Combatant,
};
use arenasim::states::play_match::AbilityType;

const WARLOCK: &str = "Team 1 Warlock #1";
const WARRIOR: &str = "Team 2 Warrior #1";

/// One Drain Life tick from a Warlock on a Warrior at `health` carrying
/// `auras`. Returns the combat log.
fn tick(health: f32, auras: Vec<Aura>) -> CombatLog {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0 / 60.0));
    world.insert_resource(time);
    let mut warrior = Combatant::new(2, 0, CharacterClass::Warrior);
    warrior.current_health = health;
    warrior.shadow_resistance = 0.0;
    let target = world
        .spawn((
            warrior,
            Transform::from_xyz(20.0, 0.0, 0.0),
            ActiveAuras { auras },
        ))
        .id();
    world.spawn((
        Combatant::new(1, 0, CharacterClass::Warlock),
        Transform::default(),
        ChannelingState {
            ability: AbilityType::DrainLife,
            duration_remaining: 5.0,
            time_until_next_tick: 0.0,
            tick_interval: 1.0,
            target,
            interrupted: false,
            interrupted_display_time: 0.0,
            ticks_applied: 0,
        },
    ));
    world.run_system_once(process_channeling).unwrap();
    world.remove_resource::<CombatLog>().unwrap()
}

fn messages(log: &CombatLog) -> Vec<&str> {
    log.entries.iter().map(|e| e.message.as_str()).collect()
}

/// The tick's structured damage, as (amount, killing blow).
fn tick_damage(log: &CombatLog) -> (f32, bool) {
    let ticks: Vec<(f32, bool)> = log
        .entries
        .iter()
        .filter_map(|e| match &e.structured_data {
            Some(StructuredEventData::Damage {
                amount,
                is_killing_blow,
                ..
            }) => Some((*amount, *is_killing_blow)),
            _ => None,
        })
        .collect();
    assert_eq!(ticks.len(), 1, "one tick: {:?}", messages(log));
    ticks[0]
}

fn aura(kind: AuraType, magnitude: f32) -> Aura {
    Aura {
        effect_type: kind,
        duration: 30.0,
        magnitude,
        break_on_damage_threshold: -1.0,
        ability_name: format!("{kind:?}"),
        ..Default::default()
    }
}

/// A damage-taken cut halves the tick, and the line says 5, not 10.
#[test]
fn a_tick_logs_the_damage_its_modifiers_left() {
    let log = tick(400.0, vec![aura(AuraType::DamageTakenReduction, 0.5)]);
    assert_eq!(
        messages(&log)[0],
        format!("{WARLOCK}'s Drain Life ticks on {WARRIOR} for 5 damage")
    );
    assert_eq!(tick_damage(&log), (5.0, false));
    // Control: no cut, the full tick.
    let log = tick(400.0, vec![]);
    assert_eq!(
        messages(&log)[0],
        format!("{WARLOCK}'s Drain Life ticks on {WARRIOR} for 10 damage")
    );
    assert_eq!(tick_damage(&log), (10.0, false));
}

/// A shield soaks part of the tick: the line names what reached health and
/// what the shield took, as a DoT tick's does, and the breakdown row counts
/// both (`combat_core::credited_damage`).
#[test]
fn a_tick_logs_the_damage_past_an_absorb() {
    let log = tick(400.0, vec![aura(AuraType::Absorb, 4.0)]);
    assert_eq!(
        messages(&log)[0],
        format!("{WARLOCK}'s Drain Life ticks on {WARRIOR} for 6 damage (4 absorbed)")
    );
    assert_eq!(tick_damage(&log), (10.0, false));
}

/// A lethal tick is the killing blow on the tick's own entry, and the death
/// follows it. (The Warlock is at full health, so its heal lands nothing and
/// logs no line: `tests/drain_life_leech.rs`.)
#[test]
fn a_lethal_tick_is_its_own_killing_blow() {
    let log = tick(5.0, vec![]);
    assert_eq!(
        messages(&log),
        vec![
            format!("{WARLOCK}'s Drain Life ticks on {WARRIOR} for 5 damage"),
            format!("{WARRIOR} has been eliminated"),
        ]
    );
    assert_eq!(tick_damage(&log), (5.0, true));
    assert_eq!(log.killing_blows(WARLOCK), 1);
    // Control: a tick it survives kills nothing.
    let log = tick(50.0, vec![]);
    assert_eq!(tick_damage(&log), (10.0, false));
    assert_eq!(log.killing_blows(WARLOCK), 0);
}

/// A Drain Life tick on a holder of Blessing of Sacrifice: the share its
/// Paladin takes is part of the tick, credited to the tick's own breakdown
/// row (AS-466), never a second "Drain Life" row. With `sacrifice` false,
/// the control: the whole tick on the Warrior.
fn shared_tick(sacrifice: bool) -> (CombatLog, f32) {
    use arenasim::states::play_match::combat_core::process_damage_follow_ups;

    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(1.0 / 60.0));
    world.insert_resource(time);
    let paladin = world
        .spawn((
            Combatant::new(2, 1, CharacterClass::Paladin),
            Transform::from_xyz(25.0, 0.0, 0.0),
            ActiveAuras::default(),
        ))
        .id();
    let mut warrior = Combatant::new(2, 0, CharacterClass::Warrior);
    warrior.shadow_resistance = 0.0;
    let mut auras = Vec::new();
    if sacrifice {
        auras.push(Aura {
            effect_type: AuraType::DamageTransfer,
            duration: 12.0,
            magnitude: 0.3,
            caster: Some(paladin),
            ability_name: "Blessing of Sacrifice".into(),
            break_on_damage_threshold: -1.0,
            ..Default::default()
        });
    }
    let target = world
        .spawn((
            warrior,
            Transform::from_xyz(20.0, 0.0, 0.0),
            ActiveAuras { auras },
        ))
        .id();
    let warlock = world
        .spawn((
            Combatant::new(1, 0, CharacterClass::Warlock),
            Transform::default(),
            ChannelingState {
                ability: AbilityType::DrainLife,
                duration_remaining: 5.0,
                time_until_next_tick: 0.0,
                tick_interval: 1.0,
                target,
                interrupted: false,
                interrupted_display_time: 0.0,
                ticks_applied: 0,
            },
        ))
        .id();
    world.run_system_once(process_channeling).unwrap();
    world.run_system_once(process_damage_follow_ups).unwrap();
    let credited = world.get::<Combatant>(warlock).unwrap().damage_dealt;
    (world.remove_resource::<CombatLog>().unwrap(), credited)
}

#[test]
fn a_shared_tick_is_one_breakdown_row() {
    let (log, credited) = shared_tick(true);
    assert!(
        log.entries
            .iter()
            .any(|e| e.message.contains("[TRANSFER]")
                && e.message.contains("(Blessing of Sacrifice")),
        "the share was passed on: {:?}",
        messages(&log)
    );
    let rows: Vec<(String, f32)> = log.damage_by_ability(WARLOCK).into_iter().collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].0, "Drain Life (tick)");
    assert!((rows[0].1 - 10.0).abs() < 1e-4, "{rows:?}");
    assert!((credited - 10.0).abs() < 1e-4, "{credited}");
    // Control: no Sacrifice, the same single row.
    let (log, _) = shared_tick(false);
    assert!(!messages(&log).iter().any(|m| m.contains("[TRANSFER]")));
    assert_eq!(
        log.damage_by_ability(WARLOCK)
            .into_iter()
            .collect::<Vec<_>>(),
        vec![("Drain Life (tick)".to_string(), 10.0)]
    );
}
