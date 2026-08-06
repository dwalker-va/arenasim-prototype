//! Durable guard: mana is charged ONLY when a completed cast actually LANDS.
//!
//! WoW-faithful rule (see `combat_core/casting.rs`): a spell's mana cost is paid
//! at the point the completed cast resolves — the projectile spawns, or an
//! instant-effect passes the alive + line-of-sight completion gates. A cast that
//! fizzles at completion (target juked out of LoS, or target dead) costs NO mana,
//! so baiting a caster into fizzles cannot drain its mana. Interrupted casts
//! never reach completion and likewise cost nothing.
//!
//! These tests drive the real `process_casting` system in a minimal Bevy App
//! (MinimalPlugins clock, gates forced open, only `process_casting` registered)
//! so they pin the wiring — where in the two-pass system the deduction happens —
//! not just the helper. Each asserts on the caster's `current_mana` delta.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::MinimalPlugins;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::combat_core::process_casting;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraType, CastEnding, CastEndingKind, CastingState,
    Combatant, GameRng, MatchCountdown,
};
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::map_geometry::ObstacleVolume;
use arenasim::states::play_match::AbilityDefinitions;
use arenasim::CharacterClass;

/// Minimal App running only `process_casting`, with gates open, a manual 1/60s
/// clock (same strategy as the headless runner), and `obstacles` as the active
/// map geometry.
fn harness_app(obstacles: Vec<ObstacleVolume>) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / 60.0,
        )))
        .insert_resource(MatchCountdown {
            time_remaining: 0.0,
            gates_opened: true,
        })
        .insert_resource(CombatLog::default())
        .insert_resource(GameRng::from_seed(0))
        .insert_resource(AbilityDefinitions::default())
        .insert_resource(ArenaDampening::default())
        .insert_resource(ActiveMapGeometry {
            // Bounds are per-map data now; these probes only exercise obstacle
            // geometry, so keep the historical octagon.
            bounds: Default::default(),
            volumes: obstacles,
            cover_anchors: Vec::new(),
        })
        .add_systems(Update, process_casting);
    app
}

fn spawn_combatant(app: &mut App, class: CharacterClass, pos: Vec3) -> Entity {
    app.world_mut()
        .spawn((Transform::from_translation(pos), Combatant::new(1, 0, class)))
        .id()
}

/// Insert a cast that completes on the next tick (`time_remaining` below one
/// frame). Fields are set directly so the tiny remaining time is explicit.
fn begin_completing_cast(app: &mut App, caster: Entity, ability: AbilityType, target: Entity) {
    app.world_mut().entity_mut(caster).insert(CastingState {
        ability,
        time_remaining: 0.001,
        target: Some(target),
        interrupted: false,
        interrupted_display_time: 0.0,
    });
}

fn mana(app: &App, entity: Entity) -> f32 {
    app.world().get::<Combatant>(entity).unwrap().current_mana
}

/// All `CastEnding` marker kinds spawned for the given caster, in spawn order.
/// `process_casting` spawns markers as bare entities (see `CastEnding` doc
/// comment) so this queries the whole world rather than reading a component
/// off the caster.
fn cast_endings(app: &mut App, caster: Entity) -> Vec<CastEndingKind> {
    app.world_mut()
        .query::<&CastEnding>()
        .iter(app.world())
        .filter(|ending| ending.caster == caster)
        .map(|ending| ending.kind)
        .collect()
}

/// A full-height pillar on the origin (shipped PillaredArena radius), straddling
/// a caster→target segment that runs along Z through the origin.
fn blocking_pillar() -> ObstacleVolume {
    ObstacleVolume::Cylinder {
        center_xz: Vec2::new(0.0, 0.0),
        radius: 2.5,
        base_y: 0.0,
        height: 5.0,
    }
}

/// Run enough frames for the sub-frame cast to tick to completion and for the
/// pass-2 resolution (and any command flush) to apply.
fn run(app: &mut App) {
    for _ in 0..5 {
        app.update();
    }
}

const FROSTBOLT_COST: f32 = 20.0;
const ICE_BARRIER_COST: f32 = 30.0;

/// (1) Projectile cast juked out of LoS at completion → the projectile never
/// spawns and NO mana is charged. This is the anti-juke-drain guarantee.
#[test]
fn projectile_fizzle_out_of_los_charges_no_mana() {
    let mut app = harness_app(vec![blocking_pillar()]);
    let mage = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(0.0, 1.0, -12.0));
    let victim = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(0.0, 1.0, 12.0));
    let before = mana(&app, mage);
    begin_completing_cast(&mut app, mage, AbilityType::Frostbolt, victim);

    run(&mut app);

    assert_eq!(
        mana(&app, mage),
        before,
        "a Frostbolt that fizzles out of LoS at completion must cost no mana"
    );
    assert_eq!(
        cast_endings(&mut app, mage),
        vec![CastEndingKind::Fizzled],
        "an LoS fizzle at completion must spawn exactly one Fizzled CastEnding"
    );
}

/// (2) Projectile cast that lands (clear LoS) → the projectile spawns and mana
/// is charged by exactly the ability's `mana_cost`. Harness sanity: same setup
/// as (1) minus the pillar, proving the test distinguishes fizzle from land.
#[test]
fn projectile_land_charges_exact_mana_cost() {
    let mut app = harness_app(Vec::new());
    let mage = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(0.0, 1.0, -12.0));
    let victim = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(0.0, 1.0, 12.0));
    let before = mana(&app, mage);
    begin_completing_cast(&mut app, mage, AbilityType::Frostbolt, victim);

    run(&mut app);

    assert!(
        (before - mana(&app, mage) - FROSTBOLT_COST).abs() < 1e-3,
        "a landed Frostbolt must charge exactly {} mana (before {}, after {})",
        FROSTBOLT_COST,
        before,
        mana(&app, mage)
    );
    assert_eq!(
        cast_endings(&mut app, mage),
        vec![CastEndingKind::Landed],
        "a landed projectile cast must spawn exactly one Landed CastEnding"
    );
}

/// (3) Instant-effect cast onto a target that is dead at completion → fizzles at
/// the `is_alive()` gate, NO mana charged. Flash Heal is instant-effect (no
/// projectile), so it exercises the instant path's dead-target short-circuit.
#[test]
fn instant_effect_dead_target_charges_no_mana() {
    let mut app = harness_app(Vec::new());
    let priest = spawn_combatant(&mut app, CharacterClass::Priest, Vec3::new(0.0, 1.0, 0.0));
    let ally = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(2.0, 1.0, 0.0));
    // Kill the heal target before the cast completes.
    app.world_mut().get_mut::<Combatant>(ally).unwrap().current_health = 0.0;
    let before = mana(&app, priest);
    begin_completing_cast(&mut app, priest, AbilityType::FlashHeal, ally);

    run(&mut app);

    assert_eq!(
        mana(&app, priest),
        before,
        "a Flash Heal completing onto a dead target must cost no mana"
    );
    assert_eq!(
        cast_endings(&mut app, priest),
        vec![CastEndingKind::Fizzled],
        "a dead-target completion must spawn exactly one Fizzled CastEnding"
    );
}

/// (4) Self-cast buff (Ice Barrier, instant-effect, target == caster) always
/// lands — caster is always alive and in LoS of itself — so it still costs mana
/// exactly as before the fix. Guards against the fix accidentally making
/// self-buffs free.
#[test]
fn self_cast_buff_still_charges_mana() {
    let mut app = harness_app(Vec::new());
    let mage = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(0.0, 1.0, 0.0));
    let before = mana(&app, mage);
    begin_completing_cast(&mut app, mage, AbilityType::IceBarrier, mage);

    run(&mut app);

    assert!(
        (before - mana(&app, mage) - ICE_BARRIER_COST).abs() < 1e-3,
        "a self-cast Ice Barrier must still charge exactly {} mana (before {}, after {})",
        ICE_BARRIER_COST,
        before,
        mana(&app, mage)
    );
    assert_eq!(
        cast_endings(&mut app, mage),
        vec![CastEndingKind::Landed],
        "a landed instant-effect self-cast must spawn exactly one Landed CastEnding"
    );
}

/// (5) Regression guard: an interrupted cast never reaches completion, so it
/// costs no mana — the behavior that already held (interrupts `continue` before
/// the completion block) and must survive the deduction move.
#[test]
fn interrupted_cast_charges_no_mana() {
    let mut app = harness_app(Vec::new());
    let mage = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(0.0, 1.0, -12.0));
    let victim = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(0.0, 1.0, 12.0));
    let before = mana(&app, mage);
    app.world_mut().entity_mut(mage).insert(CastingState {
        ability: AbilityType::Frostbolt,
        time_remaining: 0.001,
        target: Some(victim),
        interrupted: true,
        interrupted_display_time: 1.0,
    });

    run(&mut app);

    assert_eq!(
        mana(&app, mage),
        before,
        "an interrupted cast must cost no mana"
    );
    // No CastEnding marker assertion here: this scenario starts the
    // `CastingState` already `interrupted: true`, which only exercises the
    // "tick down the interrupted display" branch (casting.rs's `if
    // casting.interrupted` block) — the marker is spawned at the point the
    // interrupt is FIRST detected (the CC/Silence branches above it), not on
    // every subsequent tick of an already-interrupted cast. See
    // `stun_cancels_cast_charges_no_mana_and_spawns_interrupted_marker` below
    // for that spawn site.
}

/// (6) Stun (an incapacitating aura) cancels an in-progress cast the moment
/// `process_casting` observes it — this is the actual `Interrupted` marker
/// spawn site for the CC-cancel branch (casting.rs's `is_incapacitated` gate),
/// distinct from test (5) above which starts mid-way through an
/// already-interrupted cast. No mana is charged, matching (5)'s guarantee.
#[test]
fn stun_cancels_cast_charges_no_mana_and_spawns_interrupted_marker() {
    let mut app = harness_app(Vec::new());
    let mage = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(0.0, 1.0, -12.0));
    let victim = spawn_combatant(&mut app, CharacterClass::Mage, Vec3::new(0.0, 1.0, 12.0));
    let before = mana(&app, mage);
    begin_completing_cast(&mut app, mage, AbilityType::Frostbolt, victim);
    app.world_mut().entity_mut(mage).insert(ActiveAuras {
        auras: vec![Aura {
            effect_type: AuraType::Stun,
            duration: 5.0,
            ..Default::default()
        }],
    });

    run(&mut app);

    assert_eq!(
        mana(&app, mage),
        before,
        "a cast cancelled by an incapacitating aura must cost no mana"
    );
    assert_eq!(
        cast_endings(&mut app, mage),
        vec![CastEndingKind::Interrupted],
        "a CC-cancelled cast must spawn exactly one Interrupted CastEnding"
    );
}
