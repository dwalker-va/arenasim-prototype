//! AS-487 — a periodic effect lands every one of its ticks.
//!
//! Every periodic effect with an end — a damage over time, a heal over time,
//! a channel — ticks by one rule (`auras::periodic_tick`): a tick when its
//! timer runs out, and the final tick on the frame it ends. The timer restarts
//! on the frame each tick lands, so the ticks drift late, and without the
//! final tick the last one falls past the end. A channel used to tick without
//! it: Drain Life landed 4 of its 5 ticks.
//!
//! Every channel in `abilities.ron` is held to its whole count here, so a new
//! channel is covered with no edit. Each claim has its control: an interrupted
//! channel lands only the ticks before the interrupt, and none after.

mod common;

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::{CombatLog, StructuredEventData};
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_book::AbilityBook;
use arenasim::states::play_match::ability_config::{AbilityConfig, AbilityDefinitions};
use arenasim::states::play_match::combat_core::process_channeling;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraType, ChannelingState, Combatant, GameRng,
};
use arenasim::states::play_match::AbilityType;
use arenasim::states::play_match::{process_dot_ticks, process_hot_ticks, update_auras};
use common::staged::play;

/// The sim's step: the combat schedule runs at 60Hz (`systems.rs`).
fn dt() -> Duration {
    Duration::from_secs_f64(1.0 / 60.0)
}

/// Every channel the shipped table holds.
fn channels() -> Vec<(AbilityType, AbilityConfig)> {
    AbilityDefinitions::shipped()
        .iter()
        .filter(|(_, def)| def.is_channel())
        .map(|(ability, def)| (*ability, def.clone()))
        .collect()
}

/// The ticks a channel landed, as the frame (from 1, its first frame) each
/// landed on.
#[derive(Debug)]
struct Channelled {
    tick_frames: Vec<u32>,
    /// The frame its `ChannelingState` was gone on, if it was.
    ended_by: Option<u32>,
}

/// `ability` channelled from its start (`ChannelingState::new`, as its caster
/// starts it) by a caster of its class on a Warrior, `process_channeling` run
/// once a frame for `frames` frames. With `interrupt_at`, the channel is
/// interrupted on that frame the way `process_interrupts` interrupts one.
fn channel(
    ability: AbilityType,
    def: &AbilityConfig,
    frames: u32,
    interrupt_at: Option<u32>,
) -> Channelled {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(Time::<()>::default());
    let mut warrior = Combatant::new(2, 0, CharacterClass::Warrior);
    warrior.current_health = 10_000.0;
    warrior.max_health = 10_000.0;
    let target = world
        .spawn((
            warrior,
            Transform::from_xyz(10.0, 0.0, 0.0),
            ActiveAuras { auras: vec![] },
        ))
        .id();
    let book = AbilityBook::shipped(def.class).granting(AbilityDefinitions::shipped(), ability);
    let caster = world
        .spawn((
            Combatant::new(1, 0, def.class).with_book(book),
            Transform::default(),
            ChannelingState::new(ability, target, def),
        ))
        .id();

    let mut tick_frames = Vec::new();
    let mut ended_by = None;
    let mut seen = 0;
    for frame in 1..=frames {
        if interrupt_at == Some(frame) {
            let mut state = world
                .get_mut::<ChannelingState>(caster)
                .expect("channelling");
            state.interrupted = true;
            state.interrupted_display_time = 0.5;
        }
        world.resource_mut::<Time>().advance_by(dt());
        world.run_system_once(process_channeling).unwrap();
        let ticks = world
            .resource::<CombatLog>()
            .entries
            .iter()
            .filter(|e| matches!(e.structured_data, Some(StructuredEventData::Damage { .. })))
            .count();
        for _ in seen..ticks {
            tick_frames.push(frame);
        }
        seen = ticks;
        if ended_by.is_none() && world.get::<ChannelingState>(caster).is_none() {
            ended_by = Some(frame);
        }
    }
    Channelled {
        tick_frames,
        ended_by,
    }
}

/// The channel's whole count of ticks and its length in frames.
fn whole(def: &AbilityConfig) -> (usize, u32) {
    let duration = def.channel_duration.expect("a channel");
    let ticks = (duration / def.channel_tick_interval).round() as usize;
    (ticks, (duration * 60.0).round() as u32)
}

/// Every channel, left to run, lands its whole count of ticks: one each
/// interval, the last on the frame the channel ends, none after.
#[test]
fn an_uninterrupted_channel_lands_every_tick() {
    let channels = channels();
    assert!(
        channels.iter().any(|(a, _)| *a == AbilityType::DrainLife),
        "the shipped channels include Drain Life: {:?}",
        channels.iter().map(|(a, _)| a).collect::<Vec<_>>()
    );
    for (ability, def) in &channels {
        let (ticks, length) = whole(def);
        let interval_frames = (def.channel_tick_interval * 60.0).round() as u32;
        // Two seconds past its end: nothing more lands.
        let run = channel(*ability, def, length + 120, None);
        assert_eq!(run.tick_frames.len(), ticks, "{ability:?}: {run:?}");
        let last = *run.tick_frames.last().unwrap();
        // Within a frame of its configured end, either way: a channel that
        // ends early still lands its whole count by the final-tick rule, so
        // the count alone cannot see it.
        assert!(
            (length - 1..=length + 1).contains(&last),
            "{ability:?}'s last tick lands at its end (frame {length}): {run:?}"
        );
        assert!(
            run.tick_frames[0] >= interval_frames,
            "{ability:?}'s first tick waits one interval: {run:?}"
        );
        assert_eq!(
            run.ended_by,
            Some(last),
            "{ability:?} ends on its last tick: {run:?}"
        );
    }
}

/// The control: a channel interrupted part-way lands only the ticks before
/// the interrupt.
#[test]
fn an_interrupted_channel_lands_only_the_ticks_before_it() {
    for (ability, def) in &channels() {
        let (ticks, length) = whole(def);
        let interval_frames = (def.channel_tick_interval * 60.0).round() as u32;
        // Ten frames before the third tick is due: the third tick falls in
        // the half second the interrupted channel stays on show, and must not
        // land there.
        let at = 3 * interval_frames - 10;
        assert!(
            at < length,
            "{ability:?} is still channelling at frame {at}"
        );
        let run = channel(*ability, def, length + 120, Some(at));
        assert_eq!(run.tick_frames.len(), 2, "{ability:?}: {run:?}");
        assert!(
            run.tick_frames.iter().all(|f| *f < at),
            "{ability:?}: {run:?}"
        );
        assert!(run.tick_frames.len() < ticks, "{ability:?}: {run:?}");
    }
}

/// A 15s aura of `kind` in 3s ticks on a hurt Warrior, from a Warlock: ticked
/// by its own system and expired by `update_auras` each frame, in the order
/// the schedule runs them. The frames its ticks landed on (a damage over
/// time's damage, a heal over time's heal), and the frame it was gone on.
fn periodic_aura(kind: AuraType) -> (Vec<u32>, Option<u32>) {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(GameRng::from_seed(487));
    world.insert_resource(Time::<()>::default());
    let caster = world
        .spawn((
            Combatant::new(1, 0, CharacterClass::Warlock),
            Transform::default(),
        ))
        .id();
    let mut warrior = Combatant::new(2, 0, CharacterClass::Warrior);
    warrior.max_health = 10_000.0;
    warrior.current_health = 5_000.0;
    let aura = Aura {
        effect_type: kind,
        duration: 15.0,
        magnitude: 8.0,
        break_on_damage_threshold: -1.0,
        tick_interval: 3.0,
        time_until_next_tick: 3.0,
        caster: Some(caster),
        ability_name: "Periodic".into(),
        ..Default::default()
    };
    let target = world
        .spawn((
            warrior,
            Transform::from_xyz(10.0, 0.0, 0.0),
            ActiveAuras { auras: vec![aura] },
        ))
        .id();

    let landed = |log: &CombatLog| {
        log.entries
            .iter()
            .filter(|e| {
                matches!(
                    (&e.structured_data, kind),
                    (
                        Some(StructuredEventData::Damage { .. }),
                        AuraType::DamageOverTime
                    ) | (
                        Some(StructuredEventData::Healing { .. }),
                        AuraType::HealingOverTime
                    )
                )
            })
            .count()
    };
    let (mut tick_frames, mut gone_on, mut seen) = (Vec::new(), None, 0);
    for frame in 1..=(17 * 60) {
        world.resource_mut::<Time>().advance_by(dt());
        match kind {
            AuraType::DamageOverTime => world.run_system_once(process_dot_ticks).unwrap(),
            _ => world.run_system_once(process_hot_ticks).unwrap(),
        }
        world.run_system_once(update_auras).unwrap();
        let ticks = landed(world.resource::<CombatLog>());
        tick_frames.extend((seen..ticks).map(|_| frame));
        seen = ticks;
        let live = world
            .get::<ActiveAuras>(target)
            .is_some_and(|a| a.auras.iter().any(|a| a.effect_type == kind));
        if gone_on.is_none() && !live {
            gone_on = Some(frame);
        }
    }
    (tick_frames, gone_on)
}

/// A damage over time and a heal over time land their whole count, the last
/// on the frame they run out: the same rule as the channel.
#[test]
fn a_periodic_aura_lands_every_tick() {
    for kind in [AuraType::DamageOverTime, AuraType::HealingOverTime] {
        let (ticks, gone_on) = periodic_aura(kind);
        assert_eq!(ticks.len(), 5, "{kind:?}: {ticks:?}");
        assert!(ticks[0] >= 180, "{kind:?}'s first tick waits 3s: {ticks:?}");
        assert_eq!(gone_on, ticks.last().copied(), "{kind:?}: {ticks:?}");
    }
}

// ---------------------------------------------------------------------------
// In a match: the production schedule, the channel started as the Warlock
// starts it, a Warrior beside it whose Pummel is the control's interrupt.

const LOCK: Unit = Unit::new(1, 0);
const WARRIOR: Unit = Unit::new(2, 0);

fn config() -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec!["Warrior".into()],
        map: "BasicArena".into(),
        random_seed: Some(487),
        max_duration_secs: 7.0,
        ..Default::default()
    }
}

/// The Warlock starts a Drain Life on the Warrior beside it as the gates
/// open. Nothing else can stop it: the Warlock outlasts the Warrior's blows
/// and cannot start another cast, the Warrior has no Charge and its Pummel
/// back `pummel_in` seconds in, and the pet is benched.
fn drain_stage(pummel_in: f32) -> Stage {
    let pet = Unit::pet_of(LOCK.team, LOCK.slot);
    let mut stage = Stage::new()
        .place(LOCK, 0.0, 0.0)
        .place(WARRIOR, 2.0, 0.0)
        .clear_auras(LOCK)
        .clear_auras(WARRIOR)
        .combatant(LOCK, |c| {
            c.max_health = 5_000.0;
            c.current_health = 5_000.0;
        })
        .mana(LOCK, 0.0)
        .resource(WARRIOR, 100.0)
        .target(WARRIOR, LOCK)
        .cooldown(WARRIOR, AbilityType::Charge, 60.0)
        .cooldown(WARRIOR, AbilityType::Pummel, pummel_in)
        .place(pet, 0.0, 60.0)
        .mana(pet, 0.0);
    for held in [AbilityType::DrainLife, AbilityType::LifeTap] {
        stage = stage.cooldown(LOCK, held, 60.0);
    }
    stage.world(|world| {
        let target = WARRIOR.entity(world);
        let caster = LOCK.entity(world);
        let def = world
            .get::<Combatant>(caster)
            .expect("the Warlock")
            .book()
            .get_unchecked(&AbilityType::DrainLife)
            .clone();
        world
            .entity_mut(caster)
            .insert(ChannelingState::new(AbilityType::DrainLife, target, &def));
    })
}

/// The combat-log stamps of the Drain Life ticks and of the interrupt of it
/// (if one landed), and the app clock the stage landed on.
fn drained(pummel_in: f32) -> (Vec<f32>, Option<f32>, f32) {
    let played = play(config(), drain_stage(pummel_in));
    let stamp = |line: &str| -> f32 {
        let open = line.find('[').expect("a stamp");
        let close = line.find("s]").expect("a stamp");
        line[open + 1..close].trim().parse().expect("a stamp")
    };
    let ticks = played
        .log
        .lines()
        .filter(|l| l.contains("Drain Life ticks on"))
        .map(stamp)
        .collect();
    let interrupt = played
        .log
        .lines()
        .find(|l| l.contains("interrupts") && l.contains("Drain Life"))
        .map(stamp);
    (ticks, interrupt, played.landed().app)
}

/// Left to run, the match's Drain Life lands all 5 of its ticks, the last at
/// its end.
#[test]
fn in_a_match_drain_life_lands_all_five_ticks() {
    let (ticks, interrupt, landed) = drained(60.0);
    assert_eq!(interrupt, None, "nothing interrupted it");
    assert_eq!(ticks.len(), 5, "{ticks:?}");
    let last = *ticks.last().unwrap();
    // Within three frames of the channel's end, either way: a channel cut
    // short would still land five. (The log's stamps run about two frames
    // behind the app clock the stage landed on, and print to 0.01s: the
    // first tick, 61 frames in, is stamped 0.98s after the stage.)
    assert!(
        (last - (landed + 5.0)).abs() <= 0.05,
        "the last tick lands at the end ({}): {ticks:?}",
        landed + 5.0
    );
}

/// The control: Pummelled part-way, it lands only the ticks before the
/// Pummel.
#[test]
fn in_a_match_a_pummelled_drain_life_lands_only_the_ticks_before_it() {
    let (ticks, interrupt, landed) = drained(2.5);
    let interrupt = interrupt.expect("the Pummel interrupted it");
    assert!(
        interrupt < landed + 5.0,
        "it was interrupted mid-channel at {interrupt}: {ticks:?}"
    );
    assert!(!ticks.is_empty(), "some ticks landed before it: {ticks:?}");
    assert!(ticks.len() < 5, "{ticks:?}");
    assert!(
        ticks.iter().all(|t| *t <= interrupt),
        "{ticks:?} vs {interrupt}"
    );
}
