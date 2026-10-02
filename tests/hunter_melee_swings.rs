//! Durable guard: a Hunter swings its melee weapons inside melee range, does
//! nothing in the dead zone, and looses Auto Shot beyond it (AS-171).
//!
//! Drives the real `combat_auto_attack` system in the minimal Bevy App the
//! `dual_wield_swings.rs` harness uses (MinimalPlugins clock, gates forced
//! open, only that one system registered), so these pin the WIRING — the
//! separate melee timer, the per-hand swings, the range bands and the
//! dual-wield miss roll staying off the bow — rather than a helper in
//! isolation. Every claim is read off the `AutoAttackSwing` markers the sim
//! spawns per landed swing (which name the hand and the kind), the damage that
//! reached the victim, and the combat log.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::MinimalPlugins;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::combat_core::combat_auto_attack;
use arenasim::states::play_match::components::{
    AutoAttackKind, AutoAttackSwing, Combatant, GameRng, MatchCountdown, WeaponHand,
};
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::{AbilityDefinitions, HUNTER_DEAD_ZONE, MELEE_RANGE};
use arenasim::CharacterClass;

const TICKS_PER_SEC: u32 = 60;
const WINDOW_SECS: u32 = 60;
const WINDOW_TICKS: u32 = WINDOW_SECS * TICKS_PER_SEC;

/// The Hunter's weapons, as literals: a 3.0s bow, a 2.0s main-hand blade, a
/// 1.5s off-hand dagger. Three distinct speeds, so a swing credited to the
/// wrong clock shows up as the wrong count.
const BOW_DAMAGE: f32 = 30.0;
const BOW_SPEED: f32 = 3.0;
const MELEE_DAMAGE: f32 = 20.0;
const MELEE_SPEED: f32 = 2.0;
const OFF_DAMAGE: f32 = 10.0;
const OFF_SPEED: f32 = 1.5;

fn harness_app(seed: u64) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            1.0 / f64::from(TICKS_PER_SEC),
        )))
        .insert_resource(MatchCountdown {
            time_remaining: 0.0,
            gates_opened: true,
        })
        .insert_resource(CombatLog::default())
        .insert_resource(GameRng::from_seed(seed))
        .insert_resource(AbilityDefinitions::default())
        .insert_resource(ActiveMapGeometry {
            bounds: Default::default(),
            volumes: Vec::new(),
            cover_anchors: Vec::new(),
        })
        .add_systems(Update, combat_auto_attack);
    app
}

/// Which hand weapons the Hunter carries beside its bow.
#[derive(Clone, Copy)]
enum Melee {
    None,
    MainHand,
    DualWield,
}

/// A Hunter with a bow and, per `melee`, hand weapons. Crit is off, so every
/// landed swing deals exactly its weapon damage.
fn spawn_hunter(app: &mut App, melee: Melee) -> Entity {
    let mut hunter = Combatant::new(1, 0, CharacterClass::Hunter);
    assert_eq!(
        hunter.auto_attack_kind,
        AutoAttackKind::Shot,
        "the Hunter's live attack is its Auto Shot"
    );
    hunter.attack_damage = BOW_DAMAGE;
    hunter.weapon_speed = BOW_SPEED;
    hunter.crit_chance = 0.0;
    if matches!(melee, Melee::MainHand | Melee::DualWield) {
        hunter.melee_damage = MELEE_DAMAGE;
        hunter.melee_weapon_speed = MELEE_SPEED;
    }
    if matches!(melee, Melee::DualWield) {
        hunter.offhand_damage = OFF_DAMAGE;
        hunter.offhand_weapon_speed = OFF_SPEED;
    }
    app.world_mut()
        .spawn((Transform::from_translation(Vec3::ZERO), hunter))
        .id()
}

/// A victim `distance` yards away with a pool deep enough to outlast the
/// window, which never swings back.
fn spawn_victim(app: &mut App, hunter: Entity, distance: f32) -> Entity {
    let mut victim = Combatant::new(2, 0, CharacterClass::Warrior);
    victim.max_health = 1_000_000.0;
    victim.current_health = 1_000_000.0;
    victim.weapon_speed = 100_000.0;
    let victim = app
        .world_mut()
        .spawn((
            Transform::from_translation(Vec3::new(0.0, 0.0, distance)),
            victim,
        ))
        .id();
    app.world_mut().get_mut::<Combatant>(hunter).unwrap().target = Some(victim);
    victim
}

fn run(app: &mut App, ticks: u32) {
    for _ in 0..ticks {
        app.update();
    }
}

/// Landed swings, by (kind, hand).
#[derive(Debug, Default, PartialEq)]
struct Swings {
    shots: u32,
    melee_main: u32,
    melee_off: u32,
}

fn swings(app: &mut App) -> Swings {
    let mut markers = app.world_mut().query::<&AutoAttackSwing>();
    let mut out = Swings::default();
    for swing in markers.iter(app.world()) {
        match (swing.kind, swing.hand) {
            (AutoAttackKind::Shot, WeaponHand::Main) => out.shots += 1,
            (AutoAttackKind::Melee, WeaponHand::Main) => out.melee_main += 1,
            (AutoAttackKind::Melee, WeaponHand::Off) => out.melee_off += 1,
            other => panic!("a Hunter landed an unexpected swing {other:?}"),
        }
    }
    out
}

fn damage_taken(app: &App, victim: Entity) -> f32 {
    app.world().get::<Combatant>(victim).unwrap().damage_taken
}

/// Inside melee range the Hunter swings BOTH hands, each off its own weapon's
/// speed, and never shoots. Each hand lands about `window / speed * 0.81`
/// swings (the 19% dual-wield miss, charged to both hands).
#[test]
fn in_melee_range_both_hands_swing_and_the_bow_is_silent() {
    let mut app = harness_app(3);
    let hunter = spawn_hunter(&mut app, Melee::DualWield);
    let victim = spawn_victim(&mut app, hunter, 1.0);
    run(&mut app, WINDOW_TICKS);

    let got = swings(&mut app);
    assert_eq!(got.shots, 0, "no Auto Shot inside the dead zone: {got:?}");
    let secs = WINDOW_SECS as f32;
    for (label, landed, speed) in [
        ("main", got.melee_main, MELEE_SPEED),
        ("off", got.melee_off, OFF_SPEED),
    ] {
        let want = secs / speed * 0.81;
        assert!(
            (landed as f32 - want).abs() < want * 0.25,
            "{label} hand landed {landed} swings, expected about {want} ({got:?})"
        );
    }
    let dealt = damage_taken(&app, victim);
    assert_eq!(
        dealt,
        got.melee_main as f32 * MELEE_DAMAGE + got.melee_off as f32 * OFF_DAMAGE,
        "every landed swing dealt its own hand's weapon damage — none the bow's"
    );
    // Named as a melee swing in the log, not as a shot.
    let log = app.world().resource::<CombatLog>();
    let text: Vec<&str> = log.entries.iter().map(|e| e.message.as_str()).collect();
    assert!(
        text.iter().any(|m| m.contains("Hunter") && m.contains("'s Auto Attack hits")),
        "the melee swing is logged as an Auto Attack"
    );
    assert!(
        !text.iter().any(|m| m.contains("Auto Shot")),
        "nothing is logged as an Auto Shot in melee"
    );
}

/// Between melee range and the Auto Shot minimum the Hunter does nothing —
/// with every timer overdue, so the silence is the range gate and not a
/// window too short to fire in.
#[test]
fn the_dead_zone_fires_nothing() {
    let distance = (MELEE_RANGE + HUNTER_DEAD_ZONE) / 2.0;
    let mut app = harness_app(3);
    let hunter = spawn_hunter(&mut app, Melee::DualWield);
    let victim = spawn_victim(&mut app, hunter, distance);
    run(&mut app, 10 * TICKS_PER_SEC);

    assert_eq!(swings(&mut app), Swings::default());
    assert_eq!(damage_taken(&app, victim), 0.0);
    let c = app.world().get::<Combatant>(hunter).unwrap();
    assert!(
        c.attack_timer >= BOW_SPEED
            && c.melee_timer >= MELEE_SPEED
            && c.offhand_timer >= OFF_SPEED,
        "every hand must be overdue for the silence to mean anything: \
         bow {}, melee {}, off {}",
        c.attack_timer,
        c.melee_timer,
        c.offhand_timer
    );
}

/// Beyond the dead zone only the bow fires — and carrying melee weapons
/// changes NOTHING about it. The same seed with and without them lands the
/// same shots for the same damage and leaves the RNG in the same place, so
/// the dual-wield miss roll never reaches the Auto Shot.
#[test]
fn beyond_the_dead_zone_the_bow_fires_as_if_no_melee_weapon_were_held() {
    let run_at_range = |melee: Melee| {
        let mut app = harness_app(9);
        let hunter = spawn_hunter(&mut app, melee);
        let victim = spawn_victim(&mut app, hunter, 20.0);
        run(&mut app, WINDOW_TICKS);
        let got = swings(&mut app);
        let dealt = damage_taken(&app, victim);
        let next = app.world_mut().resource_mut::<GameRng>().random_f32();
        (got, dealt, next)
    };
    let (bare, bare_dealt, bare_rng) = run_at_range(Melee::None);
    let (armed, armed_dealt, armed_rng) = run_at_range(Melee::DualWield);

    let want_shots = WINDOW_SECS / BOW_SPEED as u32;
    assert!(
        bare.shots + 1 >= want_shots,
        "the control only loosed {} shots in {WINDOW_SECS}s at {BOW_SPEED}s",
        bare.shots
    );
    assert_eq!(bare.melee_main + bare.melee_off, 0);
    assert_eq!(armed, bare, "melee weapons changed what fires at range");
    assert_eq!(armed_dealt, bare_dealt);
    assert_eq!(
        armed_rng, bare_rng,
        "a dual-wielding Hunter's shots drew RNG a bare one's did not — the \
         dual-wield miss roll is reaching Auto Shot"
    );
}

/// The melee and ranged swings run on SEPARATE timers: a Hunter that has just
/// shot and then closes to melee swings its blade at once, off the melee
/// timer that kept building — not after the bow's interval has elapsed.
#[test]
fn the_melee_swing_does_not_wait_on_the_bow_timer() {
    let mut app = harness_app(5);
    let hunter = spawn_hunter(&mut app, Melee::MainHand);
    let victim = spawn_victim(&mut app, hunter, 20.0);
    // Run until the first shot lands, resetting the bow timer.
    let mut ticks = 0;
    while swings(&mut app).shots == 0 {
        app.update();
        ticks += 1;
        assert!(ticks < 10 * TICKS_PER_SEC, "the Hunter never shot");
    }
    let bow_timer = app.world().get::<Combatant>(hunter).unwrap().attack_timer;
    assert!(bow_timer < 0.1, "the shot reset the bow timer ({bow_timer})");

    // Step into melee range.
    app.world_mut()
        .get_mut::<Transform>(victim)
        .unwrap()
        .translation = Vec3::new(0.0, 0.0, 1.0);
    app.update();

    let got = swings(&mut app);
    assert_eq!(
        got.melee_main, 1,
        "the blade must swing on the first tick in melee range, {bow_timer}s \
         into the bow's {BOW_SPEED}s interval: {got:?}"
    );
}

/// A Hunter with no melee weapon still does nothing in melee — the sim has no
/// unarmed swing — and ticks no melee clock.
#[test]
fn a_hunter_without_a_melee_weapon_does_nothing_in_melee() {
    let mut app = harness_app(3);
    let hunter = spawn_hunter(&mut app, Melee::None);
    let victim = spawn_victim(&mut app, hunter, 1.0);
    run(&mut app, 10 * TICKS_PER_SEC);

    assert_eq!(swings(&mut app), Swings::default());
    assert_eq!(damage_taken(&app, victim), 0.0);
    let c = app.world().get::<Combatant>(hunter).unwrap();
    assert_eq!(c.melee_timer, 0.0, "no melee weapon, no melee clock");
}
