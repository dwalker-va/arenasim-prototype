//! AS-188 — a placed trap's lifetime, and what its Hunter's death does to it.
//!
//! Classic's rule (see `TRAP_LIFETIME`): a trap exists for one minute from
//! placement, and its Hunter dying does NOT remove it. Both probes kill the
//! Hunter with a trap on the ground, then walk an enemy over the trap site:
//!
//! - inside the minute, the dead Hunter's trap springs;
//! - past the minute, it has already expired unsprung, and nothing happens.
//!
//! Driven through the real `trap_system`, a tick at a time, for both trap
//! types (Freezing and Frost share the `Trap` lifecycle).

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::{
    trap_system, AuraPending, Combatant, SlowZone, Trap, TrapType, TRAP_LIFETIME,
};

/// Seconds per simulated tick — coarse enough to be quick, fine enough that
/// the lifetime boundary is bracketed tightly.
const TICK: f32 = 0.25;

const TRAP_SITE: Vec3 = Vec3::ZERO;
const FAR_AWAY: Vec3 = Vec3::new(30.0, 0.0, 0.0);

struct Arena {
    world: World,
    enemy: Entity,
    trap: Entity,
    elapsed: f32,
}

/// A dead team-1 Hunter whose trap has just landed at the origin, and a live
/// team-2 Warrior standing well clear of it.
fn dead_hunter_with_trap(trap_type: TrapType) -> Arena {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(Time::<()>::default());

    let mut hunter = Combatant::new(1, 0, CharacterClass::Hunter);
    hunter.current_health = 0.0;
    hunter.is_dead = true;
    assert!(!hunter.is_alive());
    let hunter = world
        .spawn((hunter, Transform::from_translation(Vec3::new(-20.0, 0.0, 0.0))))
        .id();
    let enemy = world
        .spawn((
            Combatant::new(2, 0, CharacterClass::Warrior),
            Transform::from_translation(FAR_AWAY),
        ))
        .id();
    let trap = world
        .spawn((
            Trap::placed(trap_type, 1, hunter),
            Transform::from_translation(TRAP_SITE),
        ))
        .id();
    Arena {
        world,
        enemy,
        trap,
        elapsed: 0.0,
    }
}

impl Arena {
    /// Run `trap_system` one tick at a time until `until` seconds have passed
    /// since the trap landed.
    fn run_until(&mut self, until: f32) {
        while self.elapsed + TICK <= until + 1e-4 {
            self.world
                .resource_mut::<Time>()
                .advance_by(Duration::from_secs_f32(TICK));
            self.world.run_system_once(trap_system).unwrap();
            self.elapsed += TICK;
        }
    }

    fn walk_enemy_onto_trap_site(&mut self) {
        self.world.get_mut::<Transform>(self.enemy).unwrap().translation = TRAP_SITE;
    }

    fn trap_exists(&self) -> bool {
        self.world.get_entity(self.trap).is_ok()
    }

    fn log_lines(&self, needle: &str) -> Vec<String> {
        self.world
            .resource::<CombatLog>()
            .entries
            .iter()
            .map(|e| e.message.clone())
            .filter(|m| m.contains(needle))
            .collect()
    }

    /// Did the trap's effect land on the enemy? Freezing Trap queues its
    /// Incapacitate on the victim; Frost Trap opens a slow zone.
    fn effect_landed(&mut self, trap_type: TrapType) -> bool {
        match trap_type {
            TrapType::Freezing => {
                let enemy = self.enemy;
                let mut q = self.world.query::<&AuraPending>();
                q.iter(&self.world)
                    .any(|p| p.target == enemy && p.aura.ability_name == "Freezing Trap")
            }
            TrapType::Frost => {
                let mut q = self.world.query::<&SlowZone>();
                q.iter(&self.world).count() > 0
            }
        }
    }
}

/// The dead Hunter's trap, 30s after landing, springs on the enemy who walks
/// onto it: Classic does not take a trap away from a Hunter who dies.
fn dead_hunters_trap_still_springs(trap_type: TrapType) {
    let mut arena = dead_hunter_with_trap(trap_type);
    arena.run_until(30.0);
    assert!(
        arena.trap_exists(),
        "{trap_type:?}: the trap is gone 30s after landing, with no enemy near it"
    );
    assert!(!arena.effect_landed(trap_type), "{trap_type:?}: sprang with no enemy near it");

    arena.walk_enemy_onto_trap_site();
    arena.run_until(30.0 + TICK);

    assert!(
        arena.effect_landed(trap_type),
        "{trap_type:?}: the dead Hunter's trap did not spring on an enemy standing on it"
    );
    assert!(!arena.trap_exists(), "{trap_type:?}: a sprung trap was not consumed");
    assert_eq!(
        arena.log_lines("triggers on").len(),
        1,
        "{trap_type:?}: expected exactly one trigger line"
    );
    assert!(arena.log_lines("expires").is_empty());
}

/// Past `TRAP_LIFETIME` the trap has expired unsprung — logged once — and an
/// enemy walking over its site afterwards is caught by nothing.
fn trap_expires_after_its_lifetime(trap_type: TrapType) {
    let mut arena = dead_hunter_with_trap(trap_type);

    // Still there, still armed, just inside the minute.
    arena.run_until(TRAP_LIFETIME - 0.5);
    assert!(
        arena.trap_exists(),
        "{trap_type:?}: expired before its {TRAP_LIFETIME}s lifetime"
    );
    assert!(arena.log_lines("expires").is_empty());

    // Past it: gone, and said so.
    arena.run_until(TRAP_LIFETIME + 0.5);
    assert!(
        !arena.trap_exists(),
        "{trap_type:?}: still on the ground {}s after landing",
        TRAP_LIFETIME + 0.5
    );
    let expired = arena.log_lines("expires unsprung");
    assert_eq!(expired.len(), 1, "{trap_type:?}: expiry lines {expired:?}");
    assert!(
        expired[0].contains(trap_type.name()),
        "{trap_type:?}: expiry line names the wrong trap: {}",
        expired[0]
    );

    // The enemy now walks over where it was: nothing springs.
    arena.walk_enemy_onto_trap_site();
    arena.run_until(TRAP_LIFETIME + 5.0);
    assert!(
        !arena.effect_landed(trap_type),
        "{trap_type:?}: an expired trap caught the enemy"
    );
    assert!(arena.log_lines("triggers on").is_empty());
}

#[test]
fn dead_hunters_freezing_trap_still_springs_inside_its_lifetime() {
    dead_hunters_trap_still_springs(TrapType::Freezing);
}

#[test]
fn dead_hunters_frost_trap_still_springs_inside_its_lifetime() {
    dead_hunters_trap_still_springs(TrapType::Frost);
}

#[test]
fn freezing_trap_expires_after_one_minute() {
    trap_expires_after_its_lifetime(TrapType::Freezing);
}

#[test]
fn frost_trap_expires_after_one_minute() {
    trap_expires_after_its_lifetime(TrapType::Frost);
}

/// The lifetime is Classic's "Trap will exist for 1 min." — pinned so a retune
/// is a deliberate edit here too.
#[test]
fn trap_lifetime_is_classics_one_minute() {
    assert_eq!(TRAP_LIFETIME, 60.0);
}
