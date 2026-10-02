//! AS-194 — only one Hunter trap is active at a time, and a thrown trap's
//! lifetime starts when it LANDS.
//!
//! Classic's rule, from every trap tooltip (Freezing Trap 1499 / 14310, Frost
//! Trap 13809): "Only one trap can be active at a time." Each is a "Summon
//! Object (slot 1)" spell and the server despawns the slot's previous object on
//! a new summon, so a new trap REPLACES the old one, and Freezing and Frost
//! share the slot (`replace_previous_trap`).
//!
//! Every probe throws through the real `spawn_trap` — the one throw site the
//! Hunter AI and the Animation Sandbox share — and drives the real
//! `move_trap_launch_projectiles` → `trap_system` chain a tick at a time, in
//! the order `add_core_combat_systems` runs them.

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::runner::TraceConfig;
use arenasim::headless::{run_headless_match_with, HeadlessMatchConfig};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::class_ai::hunter::{spawn_trap, FROST_TRAP_HELD_FOR_FREEZING};
use arenasim::states::play_match::{
    move_trap_launch_projectiles, trap_system, Combatant, Trap, TrapLaunchProjectile, TrapType,
    TRAP_LAUNCH_SPEED, TRAP_LIFETIME,
};

/// Seconds per tick. A power of two, so every multiple of it is exact in f32
/// and the lifetime boundary is bracketed to one tick.
const TICK: f32 = 1.0 / 64.0;

/// A throw long enough to fly (`> TRAP_LAUNCH_MIN_RANGE`): 1.5s in the air.
const FAR_LANDING: Vec3 = Vec3::new(30.0, 0.0, 0.0);
/// A drop short enough to be placed directly, at the Hunter's feet.
const NEAR_LANDING: Vec3 = Vec3::new(3.0, 0.0, 0.0);

struct Arena {
    world: World,
    schedule: Schedule,
    elapsed: f32,
}

impl Arena {
    /// Two Hunters on team 1 at the origin, and a team-2 Warrior far from
    /// every landing so nothing springs.
    fn new() -> (Self, Entity, Entity) {
        let mut world = World::new();
        world.insert_resource(CombatLog::default());
        world.insert_resource(Time::<()>::default());
        let hunter = world
            .spawn((
                Combatant::new(1, 0, CharacterClass::Hunter),
                Transform::from_translation(Vec3::ZERO),
            ))
            .id();
        let other_hunter = world
            .spawn((
                Combatant::new(1, 1, CharacterClass::Hunter),
                Transform::from_translation(Vec3::ZERO),
            ))
            .id();
        world.spawn((
            Combatant::new(2, 0, CharacterClass::Warrior),
            Transform::from_translation(Vec3::new(0.0, 0.0, 200.0)),
        ));
        let mut schedule = Schedule::default();
        schedule.add_systems((move_trap_launch_projectiles, trap_system).chain());
        (
            Arena {
                world,
                schedule,
                elapsed: 0.0,
            },
            hunter,
            other_hunter,
        )
    }

    /// Throw (or drop) a trap from the origin through the shared `spawn_trap`,
    /// applying its commands — the replacement included — as a frame's flush
    /// would.
    fn throw(&mut self, owner: Entity, landing: Vec3, trap_type: TrapType) {
        self.world
            .run_system_once(move |mut commands: Commands| {
                spawn_trap(&mut commands, owner, 1, Vec3::ZERO, landing, trap_type);
            })
            .unwrap();
    }

    /// One frame: advance the clock, run the chain, then stamp the log's
    /// clock as `combat_auto_attack` does after them.
    fn tick(&mut self) {
        self.world
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(TICK));
        self.schedule.run(&mut self.world);
        self.elapsed += TICK;
        self.world.resource_mut::<CombatLog>().match_time = self.elapsed;
    }

    fn run_for(&mut self, secs: f32) {
        let end = self.elapsed + secs;
        while self.elapsed + TICK <= end + 1e-4 {
            self.tick();
        }
    }

    /// The traps on the ground, by owner and type.
    fn traps(&mut self) -> Vec<(Entity, TrapType)> {
        let mut q = self.world.query::<&Trap>();
        q.iter(&self.world)
            .map(|t| (t.owner, t.trap_type))
            .collect()
    }

    /// The traps in flight, by owner and type.
    fn in_flight(&mut self) -> Vec<(Entity, TrapType)> {
        let mut q = self.world.query::<&TrapLaunchProjectile>();
        q.iter(&self.world)
            .map(|p| (p.owner, p.trap_type))
            .collect()
    }

    /// Log lines containing `needle`, with their timestamps.
    fn log_lines(&self, needle: &str) -> Vec<(f32, String)> {
        self.world
            .resource::<CombatLog>()
            .entries
            .iter()
            .filter(|e| e.message.contains(needle))
            .map(|e| (e.timestamp, e.message.clone()))
            .collect()
    }
}

// ----------------------------------------------------------------------------
// The lifetime starts at landing
// ----------------------------------------------------------------------------

/// A THROWN trap expires `TRAP_LIFETIME` after its `lands at` line, to the
/// tick. The flight is not charged against the lifetime: a trap that spent its
/// 1.5s arc counting down would expire 1.5s early, ninety-six ticks outside
/// the bracket.
fn thrown_trap_expires_a_lifetime_after_it_lands(trap_type: TrapType) {
    let (mut arena, hunter, _) = Arena::new();
    arena.throw(hunter, FAR_LANDING, trap_type);
    let flight = FAR_LANDING.length() / TRAP_LAUNCH_SPEED;
    assert_eq!(
        arena.in_flight(),
        vec![(hunter, trap_type)],
        "{trap_type:?}: not thrown"
    );

    arena.run_for(flight + TRAP_LIFETIME + 1.0);

    let landed = arena.log_lines("lands at");
    let expired = arena.log_lines("expires unsprung");
    assert_eq!(landed.len(), 1, "{trap_type:?}: landing lines {landed:?}");
    assert_eq!(expired.len(), 1, "{trap_type:?}: expiry lines {expired:?}");
    assert!(
        (landed[0].0 - flight).abs() <= TICK,
        "{trap_type:?}: landed at {}s, expected after its {flight}s flight",
        landed[0].0
    );
    let lived = expired[0].0 - landed[0].0;
    assert!(
        (lived - TRAP_LIFETIME).abs() <= TICK,
        "{trap_type:?}: expired {lived}s after landing, expected {TRAP_LIFETIME}s to the tick ({TICK}s)"
    );
    assert!(arena.traps().is_empty());
}

#[test]
fn thrown_freezing_trap_expires_a_lifetime_after_it_lands() {
    thrown_trap_expires_a_lifetime_after_it_lands(TrapType::Freezing);
}

#[test]
fn thrown_frost_trap_expires_a_lifetime_after_it_lands() {
    thrown_trap_expires_a_lifetime_after_it_lands(TrapType::Frost);
}

// ----------------------------------------------------------------------------
// One active trap
// ----------------------------------------------------------------------------

/// A Freezing Trap thrown while the Hunter's Frost Trap is on the ground
/// removes the Frost Trap at once, says so, and lands alone. Freezing and Frost
/// share the limit.
#[test]
fn a_new_trap_replaces_the_hunters_trap_on_the_ground() {
    let (mut arena, hunter, _) = Arena::new();
    arena.throw(hunter, NEAR_LANDING, TrapType::Frost);
    arena.run_for(5.0);
    assert_eq!(arena.traps(), vec![(hunter, TrapType::Frost)]);

    arena.throw(hunter, FAR_LANDING, TrapType::Freezing);
    assert!(
        arena.traps().is_empty(),
        "the Frost Trap is still on the ground after the Freezing Trap was thrown"
    );
    let replaced = arena.log_lines("is replaced by");
    assert_eq!(replaced.len(), 1, "replacement lines {replaced:?}");
    assert!(
        replaced[0]
            .1
            .contains("Frost Trap is replaced by its Freezing Trap"),
        "{}",
        replaced[0].1
    );

    arena.run_for(2.0);
    assert_eq!(arena.traps(), vec![(hunter, TrapType::Freezing)]);
    assert!(
        arena.log_lines("expires").is_empty(),
        "a replaced trap is not an expiry"
    );
}

/// A trap still in the air is the Hunter's trap too: a newer one replaces it
/// before it lands, so an older throw never lands on top of a newer one.
#[test]
fn a_new_trap_replaces_the_hunters_trap_in_flight() {
    let (mut arena, hunter, _) = Arena::new();
    arena.throw(hunter, FAR_LANDING, TrapType::Freezing);
    arena.run_for(0.5);
    assert_eq!(arena.in_flight(), vec![(hunter, TrapType::Freezing)]);

    arena.throw(hunter, NEAR_LANDING, TrapType::Frost);
    assert!(
        arena.in_flight().is_empty(),
        "the Freezing Trap is still in flight"
    );
    assert_eq!(arena.traps(), vec![(hunter, TrapType::Frost)]);

    // Well past when the Freezing Trap would have landed.
    arena.run_for(3.0);
    assert_eq!(arena.traps(), vec![(hunter, TrapType::Frost)]);
    assert!(
        arena.log_lines("lands at").is_empty(),
        "the replaced throw landed"
    );
    assert_eq!(
        arena
            .log_lines("Freezing Trap is replaced by its Frost Trap")
            .len(),
        1
    );
}

/// The same trap type replaces itself: one slot, whatever fills it.
#[test]
fn a_freezing_trap_replaces_the_hunters_freezing_trap() {
    let (mut arena, hunter, _) = Arena::new();
    arena.throw(hunter, NEAR_LANDING, TrapType::Freezing);
    arena.run_for(1.0);
    arena.throw(hunter, -NEAR_LANDING, TrapType::Freezing);
    arena.run_for(1.0);
    let mut q = arena.world.query::<(&Trap, &Transform)>();
    let traps: Vec<Vec3> = q.iter(&arena.world).map(|(_, t)| t.translation).collect();
    assert_eq!(
        traps,
        vec![-NEAR_LANDING],
        "the older Freezing Trap survived"
    );
}

/// The limit is per HUNTER, not per team: a teammate Hunter's trap is
/// untouched by this Hunter's throw.
#[test]
fn another_hunters_trap_is_untouched() {
    let (mut arena, hunter, other) = Arena::new();
    arena.throw(other, NEAR_LANDING, TrapType::Frost);
    arena.run_for(1.0);
    arena.throw(hunter, -NEAR_LANDING, TrapType::Freezing);
    arena.run_for(1.0);
    let mut traps = arena.traps();
    traps.sort_by_key(|(e, _)| e.to_bits());
    let mut expected = vec![(hunter, TrapType::Freezing), (other, TrapType::Frost)];
    expected.sort_by_key(|(e, _)| e.to_bits());
    assert_eq!(traps, expected);
    assert!(arena.log_lines("is replaced by").is_empty());
}

// ----------------------------------------------------------------------------
// The AI holds a Frost Trap that would replace its own Freezing Trap
// ----------------------------------------------------------------------------

/// Hunter+Shaman v Warrior+Priest, BasicArena, seed 0: at 17.97s the Hunter,
/// its Freezing Trap still unsprung, used to drop a Frost Trap that — under
/// the rule — would have taken the Freezing Trap off the ground. It holds it
/// instead, and the trace says why. Across the whole match no Freezing Trap is
/// ever replaced by a Frost Trap.
#[test]
fn the_hunter_holds_a_frost_trap_that_would_replace_its_freezing_trap() {
    let dir = tempfile::tempdir().unwrap();
    let trace_path = dir.path().join("trace.jsonl");
    let log_path = dir.path().join("match.txt");
    let cfg = HeadlessMatchConfig {
        team1: vec!["Hunter".into(), "Shaman".into()],
        team2: vec!["Warrior".into(), "Priest".into()],
        max_duration_secs: 180.0,
        random_seed: Some(0),
        output_path: Some(log_path.to_string_lossy().into_owned()),
        ..Default::default()
    };
    run_headless_match_with(
        cfg,
        false,
        Some(TraceConfig {
            output_path: trace_path.clone(),
        }),
    )
    .expect("headless match");
    let trace = std::fs::read_to_string(&trace_path).unwrap();

    let held = trace
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["actor"]["class"] == "Hunter")
        .flat_map(|v| v["candidates"].as_array().cloned().unwrap_or_default())
        .filter(|c| {
            c["ability"] == "FrostTrap"
                && c["reason"]["PreconditionUnmet"]["note"] == FROST_TRAP_HELD_FOR_FREEZING
        })
        .count();
    assert!(
        held > 0,
        "the guard never held a Frost Trap: the probe is vacuous"
    );

    let log = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        log.contains("Frost Trap"),
        "no Frost Trap in the match: the probe is vacuous"
    );
    assert!(
        !log.contains("Freezing Trap is replaced by its Frost Trap"),
        "a Frost Trap replaced the Hunter's own Freezing Trap"
    );
}
