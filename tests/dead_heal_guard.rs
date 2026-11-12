//! AS-454 — a heal that resolves on a dead unit lands nothing.
//!
//! A heal can resolve a frame or more after it was chosen, or later in the
//! frame of the blow that killed its target. Before the guard at the top of
//! `combat_core::apply_healing`, such a heal raised the corpse back above 0
//! health: the unit stayed dead (`is_dead`), but every reader of its health —
//! the results screen's final health, a healer's `current_health > 0` target
//! filter — saw a living unit. Two sites carried no guard of their own, and
//! each is staged here as a pair, the heal on a corpse and the same heal on a
//! living unit:
//!
//! - a killing-blow save's heal (Guardian Spirit's shape), a follow-up that
//!   lands at the end of the frame, after a second lethal blow in that frame
//!   has killed its holder;
//! - Drain Life's heal to its channeler, queued as the tick is chosen, after
//!   the channeler's own victim's Drain Life killed it in the same pass.
//!
//! The function's own pair pins the guard at a unit at 0 health that no site
//! has marked dead yet.

mod common;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::combat_core::apply_healing;
use arenasim::states::play_match::components::{
    ArenaDampening, Aura, AuraType, ChannelingState, Combatant,
};
use arenasim::states::play_match::AbilityType;
use common::staged::{play, Played};

/// Drain Life's tick damage and its heal to the channeler (`abilities.ron`).
const DRAIN_TICK: f32 = 10.0;
const DRAIN_HEAL: f32 = 10.0;

/// The save's heal, as a fraction of the holder's maximum health.
const SAVE_HEAL: f32 = 0.25;

/// Health one Drain Life tick kills from.
const LOW: f32 = 5.0;

fn config(team1: &[&str], team2: &[&str]) -> HeadlessMatchConfig {
    HeadlessMatchConfig {
        team1: team1.iter().map(|c| c.to_string()).collect(),
        team2: team2.iter().map(|c| c.to_string()).collect(),
        map: "BasicArena".into(),
        random_seed: Some(454),
        max_duration_secs: 5.0,
        ..Default::default()
    }
}

/// `who` stands at `x` on the centre line with `health` points, no shadow
/// resistance (so a tick is its exact number) and no mana (so it casts
/// nothing before the frame's ticks resolve).
fn ready(stage: Stage, who: Unit, x: f32, health: f32) -> Stage {
    stage
        .place(who, x, 0.0)
        .clear_auras(who)
        .mana(who, 0.0)
        .combatant(who, move |c| {
            c.current_health = health;
            c.shadow_resistance = 0.0;
        })
}

/// `pet`, out of the way and out of mana: no Spell Lock on a channel, no
/// Devour Magic on a save, no swing at anyone this frame.
fn benched(stage: Stage, pet: Unit, x: f32) -> Stage {
    stage.place(pet, x, 60.0).mana(pet, 0.0)
}

/// `caster` is channeling Drain Life on `target`, its next tick due this
/// frame.
fn draining(stage: Stage, caster: Unit, target: Unit) -> Stage {
    stage.world(move |world| {
        let target = target.entity(world);
        let caster = caster.entity(world);
        world.entity_mut(caster).insert(ChannelingState {
            ability: AbilityType::DrainLife,
            duration_remaining: 5.0,
            time_until_next_tick: 0.0,
            tick_interval: 1.0,
            target,
            interrupted: false,
            interrupted_display_time: 0.0,
            ticks_applied: 0,
        });
    })
}

/// The combatant in `slot` of `team` on the first staged frame — the frame
/// the ticks resolved on — as (health, alive).
fn on_tick_frame(played: &Played, team: u8, slot: u8) -> (f32, bool) {
    let frame = played.staged_frames().next().expect("a staged frame");
    let (_, c) = frame
        .combatants
        .iter()
        .find(|(_, c)| c.team == team && c.slot == slot && !c.is_pet)
        .expect("the combatant");
    (c.current_health, c.alive)
}

/// No frame shows a dead combatant above 0 health.
fn assert_corpses_stay_at_zero(played: &Played) {
    for frame in &played.frames {
        for (e, c) in &frame.combatants {
            assert!(
                c.alive || c.current_health == 0.0,
                "{e:?} ({:?}, team {}) is dead at {} health at {:.3}s",
                c.class,
                c.team,
                c.current_health,
                frame.sim_time
            );
        }
    }
}

// ---------------------------------------------------------------------------
// A save's heal, in flight to the end of the frame
// ---------------------------------------------------------------------------

const PRIEST: Unit = Unit::new(1, 0);
const LOCK_A: Unit = Unit::new(2, 0);
const LOCK_B: Unit = Unit::new(2, 1);

/// The Priest at `LOW` health holding a save, drained by `drainers`.
fn save_stage(drainers: &[Unit]) -> Stage {
    let mut stage = ready(Stage::new(), PRIEST, 0.0, LOW).aura(
        PRIEST,
        Aura {
            effect_type: AuraType::KillingBlowSave,
            duration: 30.0,
            magnitude: SAVE_HEAL,
            break_on_damage_threshold: -1.0,
            ability_name: "Guardian Spirit".into(),
            ..Default::default()
        },
    );
    for (i, lock) in [LOCK_A, LOCK_B].into_iter().enumerate() {
        let x = if i == 0 { 15.0 } else { -15.0 };
        stage = ready(stage, lock, x, 400.0);
        stage = benched(stage, Unit::pet_of(2, lock.slot), x);
    }
    drainers
        .iter()
        .fold(stage, |s, &lock| draining(s, lock, PRIEST))
}

fn priest_result(played: &Played) -> (f32, bool) {
    let p = &played.result.team1_combatants[0];
    (p.final_health, p.survived)
}

/// Two lethal ticks in one frame: the save takes the first, the second kills
/// the Priest, and the save's heal — landed at the frame's end — finds a
/// corpse and lands nothing.
#[test]
fn a_saves_heal_on_a_holder_killed_in_its_frame_lands_nothing() {
    let played = play(
        config(&["Priest"], &["Warlock", "Warlock"]),
        save_stage(&[LOCK_A, LOCK_B]),
    );
    // The situation: the save fired, and the Priest still died that frame.
    assert!(
        played
            .log
            .contains("[SAVED] Guardian Spirit prevents a killing blow on Team 1 Priest #1"),
        "the save fired:\n{}",
        played.log
    );
    assert_eq!(played.result.winner, Some(2), "the Priest died");
    assert_eq!(on_tick_frame(&played, 1, 0), (0.0, false));
    assert_eq!(priest_result(&played), (0.0, false));
    assert!(
        played
            .log
            .contains("[SAVED] Guardian Spirit heals Team 1 Priest #1 for 0"),
        "{}",
        played.log
    );
    assert_corpses_stay_at_zero(&played);
}

/// The control: one lethal tick. The save takes it and its heal lands on the
/// living Priest.
#[test]
fn a_saves_heal_on_a_living_holder_lands() {
    let played = play(
        config(&["Priest"], &["Warlock", "Warlock"]),
        save_stage(&[LOCK_A]),
    );
    assert!(played
        .log
        .contains("[SAVED] Guardian Spirit prevents a killing blow on Team 1 Priest #1"));
    let frame = played.staged_frames().next().expect("a staged frame");
    let max = frame
        .combatants
        .values()
        .find(|c| c.team == 1 && c.slot == 0)
        .expect("the Priest")
        .max_health;
    let (health, alive) = on_tick_frame(&played, 1, 0);
    assert!(alive, "the save kept the Priest alive");
    assert_eq!(health, LOW + max * SAVE_HEAL, "the save's heal landed");
    assert_corpses_stay_at_zero(&played);
}

// ---------------------------------------------------------------------------
// Drain Life's heal, queued before its channeler was killed
// ---------------------------------------------------------------------------

const LOCK_1: Unit = Unit::new(1, 0);
const LOCK_2: Unit = Unit::new(2, 0);

/// Two Warlocks at `health`, each draining the other, both ticks due now.
fn mutual_drain(health: f32) -> Stage {
    let mut stage = Stage::new();
    for (lock, x) in [(LOCK_1, -10.0), (LOCK_2, 10.0)] {
        stage = ready(stage, lock, x, health);
        stage = benched(stage, Unit::pet_of(lock.team, lock.slot), x);
    }
    let stage = draining(stage, LOCK_1, LOCK_2);
    draining(stage, LOCK_2, LOCK_1)
}

/// Each tick kills the other Warlock in the same pass, so both Drain Life
/// heals resolve on a corpse: the match is a draw at 0 health a side.
#[test]
fn drain_lifes_heal_to_a_channeler_killed_in_its_pass_lands_nothing() {
    let played = play(config(&["Warlock"], &["Warlock"]), mutual_drain(LOW));
    // The situation: both died on the frame both ticks resolved.
    assert_eq!(played.result.winner, None, "both Warlocks died");
    assert_eq!(
        played.log.matches("Drain Life ticks on").count(),
        2,
        "both ticks resolved:\n{}",
        played.log
    );
    assert_eq!(on_tick_frame(&played, 1, 0), (0.0, false));
    assert_eq!(on_tick_frame(&played, 2, 0), (0.0, false));
    for side in [
        &played.result.team1_combatants,
        &played.result.team2_combatants,
    ] {
        assert_eq!((side[0].final_health, side[0].survived), (0.0, false));
    }
    assert_corpses_stay_at_zero(&played);
}

/// The control: each Warlock survives the other's tick, and its own heal
/// lands — health back where it was.
#[test]
fn drain_lifes_heal_to_a_living_channeler_lands() {
    let start = 50.0;
    let played = play(config(&["Warlock"], &["Warlock"]), mutual_drain(start));
    for team in [1, 2] {
        assert_eq!(
            on_tick_frame(&played, team, 0),
            (start - DRAIN_TICK + DRAIN_HEAL, true),
            "team {team}'s Warlock took the tick and its heal"
        );
    }
    assert_corpses_stay_at_zero(&played);
}

// ---------------------------------------------------------------------------
// The guard itself
// ---------------------------------------------------------------------------

/// Heal `combatant` for `amount` through `apply_healing`; returns the
/// effective healing and the health after.
fn heal(combatant: Combatant, amount: f32) -> (f32, f32) {
    let mut world = World::new();
    world.insert_resource(ArenaDampening::default());
    let e = world.spawn(combatant).id();
    let healed = world
        .run_system_once(
            move |mut commands: Commands,
                  dampening: Res<ArenaDampening>,
                  mut q: Query<&mut Combatant>| {
                let mut c = q.get_mut(e).expect("the combatant");
                apply_healing(&mut commands, e, &mut c, None, &dampening, amount)
            },
        )
        .expect("ran");
    (healed, world.get::<Combatant>(e).unwrap().current_health)
}

fn priest(health: f32, marked_dead: bool) -> Combatant {
    let mut c = Combatant::new(1, 0, CharacterClass::Priest);
    c.max_health = 400.0;
    c.current_health = health;
    c.is_dead = marked_dead;
    c
}

/// A unit at 0 health is dead whether or not a site has marked it yet: a
/// heal later in the frame of its killing blow does not raise it.
#[test]
fn apply_healing_refuses_a_unit_at_zero_health() {
    assert_eq!(heal(priest(0.0, true), 100.0), (0.0, 0.0), "marked dead");
    assert_eq!(
        heal(priest(0.0, false), 100.0),
        (0.0, 0.0),
        "not yet marked"
    );
    // Control: a living unit is healed.
    assert_eq!(heal(priest(1.0, false), 100.0), (100.0, 101.0));
}
