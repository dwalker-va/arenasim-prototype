//! Probes for a dual-wielder's two weapon sockets following the sim's two
//! swing timers (`consume_swing_signals` and `animate_weapon_swings` in
//! `rendering/effects/weapon_swing.rs`).
//!
//! The sim swings each hand on its own clock (`attack_timer` /
//! `offhand_timer`), and every landed swing's `AutoAttackSwing` marker names
//! the hand it came from. The renderer must follow that rather than guess:
//! the dagger in the hand that swung strikes, and each dagger telegraphs off
//! its own hand's timer. A renderer that alternated daggers per landed swing
//! would show the wrong dagger striking whenever the hands' speeds differ.
//!
//! Every claim is read off the socket's `GlobalTransform` — the pose that
//! reaches the screen — never off a stored field. The sockets rest at
//! identity under an unrotated owner facing its victim straight down +Z, so
//! any rotation away from identity is the swing pose itself.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::components::{
    AutoAttackKind, AutoAttackSwing, Combatant, SwingStyle, VisualBody, WeaponHand, WeaponKind,
    WeaponSocket,
};
use arenasim::states::play_match::{animate_weapon_swings, consume_swing_signals};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(50);
/// Frames sampled per probe: long enough for a release stroke to reach full
/// extension and for an eased windup to reach depth.
const FRAMES: usize = 10;
/// A pose this far from rest (radians) is a swing, not float noise.
const MOVED: f32 = 0.05;

struct Rogue {
    unit: Entity,
    main: Entity,
    off: Entity,
}

fn harness() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        TransformPlugin,
    ));
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_asset::<Image>();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
    app.add_systems(
        Update,
        (consume_swing_signals, animate_weapon_swings).chain(),
    );
    app
}

fn spawn_socket(app: &mut App, owner: Entity, body: Entity, hand: WeaponHand) -> Entity {
    let socket = app
        .world_mut()
        .spawn((
            WeaponSocket {
                kind: WeaponKind::Dagger,
                hand,
                owner,
                rest: Transform::IDENTITY,
                release_t: None,
                aim: Vec3::ZERO,
                yaw_local: 0.0,
                prev_owner_yaw: 0.0,
                windup_s: 0.0,
                swing_style: SwingStyle::Auto,
                last_s: 0.0,
            },
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    app.world_mut().entity_mut(body).add_child(socket);
    socket
}

/// A Rogue in the hierarchy `spawn_combatant` builds for it — a dagger in
/// each hand — targeting a victim 2yd ahead, inside melee reach. `armed`
/// puts a weapon in the sim's off hand; without it the Rogue single-wields
/// and the off-hand dagger is a prop.
fn spawn_rogue(app: &mut App, armed: bool) -> Rogue {
    let victim = app
        .world_mut()
        .spawn((
            Transform::from_xyz(0.0, 1.0, 2.0),
            Combatant::new(1, 0, CharacterClass::Priest),
        ))
        .id();
    let mut combatant = Combatant::new(0, 0, CharacterClass::Rogue);
    // Out of the opener: a stealthed Rogue never telegraphs a swing.
    combatant.stealthed = false;
    combatant.attack_speed = 1.0;
    if armed {
        combatant.offhand_damage = 10.0;
        combatant.offhand_speed = 2.0;
    }
    combatant.target = Some(victim);
    let unit = app
        .world_mut()
        .spawn((Transform::from_xyz(0.0, 1.0, 0.0), combatant))
        .id();
    let body = app
        .world_mut()
        .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
        .id();
    app.world_mut().entity_mut(unit).add_child(body);
    let main = spawn_socket(app, unit, body, WeaponHand::Main);
    let off = spawn_socket(app, unit, body, WeaponHand::Off);
    Rogue { unit, main, off }
}

fn set_timers(app: &mut App, unit: Entity, main: f32, off: f32) {
    let mut c = app.world_mut().get_mut::<Combatant>(unit).unwrap();
    c.attack_timer = main;
    c.offhand_timer = off;
}

fn swing(app: &mut App, rogue: &Rogue, hand: WeaponHand) {
    let target = app
        .world()
        .get::<Combatant>(rogue.unit)
        .unwrap()
        .target
        .unwrap();
    app.world_mut().spawn(AutoAttackSwing {
        attacker: rogue.unit,
        target,
        kind: AutoAttackKind::Melee,
        is_crit: false,
        hand,
    });
}

/// The world rotation of a socket away from its rest pose.
fn pose_angle(app: &App, socket: Entity) -> f32 {
    app.world()
        .get::<GlobalTransform>(socket)
        .unwrap()
        .compute_transform()
        .rotation
        .angle_between(Quat::IDENTITY)
}

/// The furthest each socket (main, off) swings from rest over `FRAMES` frames.
fn max_poses(app: &mut App, rogue: &Rogue) -> (f32, f32) {
    let (mut main, mut off) = (0.0f32, 0.0f32);
    for _ in 0..FRAMES {
        app.update();
        main = main.max(pose_angle(app, rogue.main));
        off = off.max(pose_angle(app, rogue.off));
    }
    (main, off)
}

/// An off-hand swing strikes with the off-hand dagger, and only with it.
#[test]
fn an_off_hand_swing_strikes_with_the_off_hand_dagger() {
    let mut app = harness();
    let rogue = spawn_rogue(&mut app, true);
    swing(&mut app, &rogue, WeaponHand::Off);
    let (main, off) = max_poses(&mut app, &rogue);
    assert!(off > MOVED, "the off-hand dagger must strike: {off} rad");
    assert!(
        main < MOVED,
        "the main-hand dagger must stay at rest: {main} rad"
    );
}

/// A main-hand swing strikes with the main-hand dagger, and only with it.
#[test]
fn a_main_hand_swing_strikes_with_the_main_hand_dagger() {
    let mut app = harness();
    let rogue = spawn_rogue(&mut app, true);
    swing(&mut app, &rogue, WeaponHand::Main);
    let (main, off) = max_poses(&mut app, &rogue);
    assert!(main > MOVED, "the main-hand dagger must strike: {main} rad");
    assert!(
        off < MOVED,
        "the off-hand dagger must stay at rest: {off} rad"
    );
}

/// Consecutive swings from ONE hand keep striking with that hand. A renderer
/// that alternated daggers per landed swing would hand the second of two
/// main-hand swings to the off-hand dagger — which is exactly what a faster
/// weapon in one hand produces.
#[test]
fn repeated_swings_from_one_hand_never_pass_to_the_other() {
    let mut app = harness();
    let rogue = spawn_rogue(&mut app, true);
    swing(&mut app, &rogue, WeaponHand::Main);
    max_poses(&mut app, &rogue);
    swing(&mut app, &rogue, WeaponHand::Main);
    let (main, off) = max_poses(&mut app, &rogue);
    assert!(
        main > MOVED,
        "the second main-hand swing must strike: {main} rad"
    );
    assert!(
        off < MOVED,
        "the off-hand dagger must not take it: {off} rad"
    );
}

/// Two hands landing on one tick strike together.
#[test]
fn same_tick_swings_strike_with_both_daggers() {
    let mut app = harness();
    let rogue = spawn_rogue(&mut app, true);
    swing(&mut app, &rogue, WeaponHand::Main);
    swing(&mut app, &rogue, WeaponHand::Off);
    let (main, off) = max_poses(&mut app, &rogue);
    assert!(
        main > MOVED && off > MOVED,
        "both must strike: main {main}, off {off}"
    );
}

/// Each dagger telegraphs off its OWN hand's timer: an off-hand swing that is
/// about to land raises the off-hand dagger while the main hand, far from its
/// swing, stays at rest.
#[test]
fn the_off_hand_winds_up_on_its_own_timer() {
    let mut app = harness();
    let rogue = spawn_rogue(&mut app, true);
    // Off-hand interval is 1/2.0 = 0.5s; the main hand's is 1.0s.
    set_timers(&mut app, rogue.unit, 0.0, 0.49);
    let (main, off) = max_poses(&mut app, &rogue);
    assert!(off > MOVED, "the off-hand dagger must wind up: {off} rad");
    assert!(
        main < MOVED,
        "the main-hand dagger must stay at rest: {main} rad"
    );
}

/// And the converse: a main-hand swing about to land does not raise the
/// off-hand dagger.
#[test]
fn the_main_hand_winds_up_on_its_own_timer() {
    let mut app = harness();
    let rogue = spawn_rogue(&mut app, true);
    set_timers(&mut app, rogue.unit, 0.99, 0.0);
    let (main, off) = max_poses(&mut app, &rogue);
    assert!(
        main > MOVED,
        "the main-hand dagger must wind up: {main} rad"
    );
    assert!(
        off < MOVED,
        "the off-hand dagger must stay at rest: {off} rad"
    );
}

/// A Rogue with no weapon in its sim off hand swings nothing there, so the
/// second dagger it is drawn holding never telegraphs — whatever the unused
/// off-hand timer reads.
#[test]
fn an_unarmed_off_hand_never_winds_up() {
    let mut app = harness();
    let rogue = spawn_rogue(&mut app, false);
    set_timers(&mut app, rogue.unit, 0.0, 10.0);
    let (_, off) = max_poses(&mut app, &rogue);
    assert!(
        off < MOVED,
        "an unarmed off hand must stay at rest: {off} rad"
    );
}
