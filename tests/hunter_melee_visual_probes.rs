//! Probes for a Hunter carrying melee weapons beside its bow (AS-171):
//! `consume_swing_signals` and `animate_weapon_swings` in
//! `rendering/effects/weapon_swing.rs`.
//!
//! Such a Hunter holds TWO main-hand models — the bow and the blade — plus an
//! off-hand dagger, and shows one set at a time (`WeaponSetSwap`): the hand
//! weapons once its target closes to melee reach, the bow once the target is
//! back at the Auto Shot minimum, and whichever it had in the dead zone
//! between. A melee swing must strike with the blade and never the bow, a Shot
//! must loose the bow and never swing the blade, and each set telegraphs off
//! its OWN sim clock (`melee_timer` / `attack_timer`).
//!
//! Swing claims are read off the socket's `GlobalTransform` — the pose that
//! reaches the screen — as in `dual_wield_visual_probes.rs`; the swap claims
//! off each socket's `Visibility`, the component the renderer hides it by.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::components::{
    AutoAttackKind, AutoAttackSwing, Combatant, SwingStyle, VisualBody, WeaponHand, WeaponKind,
    WeaponSet, WeaponSetSwap, WeaponSocket,
};
use arenasim::states::play_match::{
    animate_weapon_swings, consume_swing_signals, HUNTER_DEAD_ZONE, MELEE_RANGE,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(50);
const FRAMES: usize = 10;
/// A pose this far from rest (radians plus yards) is a swing, not float noise.
const MOVED: f32 = 0.05;

const BOW_SPEED: f32 = 3.0;
const MELEE_SPEED: f32 = 2.0;

struct Hunter {
    unit: Entity,
    victim: Entity,
    bow: Entity,
    blade: Entity,
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

fn spawn_socket(
    app: &mut App,
    owner: Entity,
    body: Entity,
    kind: WeaponKind,
    hand: WeaponHand,
) -> Entity {
    let swap = WeaponSetSwap::new(WeaponSet::of(kind));
    let visibility = if swap.shown() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    let socket = app
        .world_mut()
        .spawn((
            WeaponSocket {
                kind,
                hand,
                owner,
                rest: Transform::IDENTITY,
                release_t: None,
                aim: Vec3::ZERO,
                yaw_local: 0.0,
                prev_owner_yaw: 0.0,
                windup_s: 0.0,
                swing_style: SwingStyle::Auto,
                stroke_interval: 0.0,
                last_s: 0.0,
            },
            swap,
            Transform::default(),
            visibility,
        ))
        .id();
    app.world_mut().entity_mut(body).add_child(socket);
    socket
}

/// A Hunter in the hierarchy `spawn_combatant` builds for one with a bow, a
/// main-hand blade and an off-hand dagger, targeting a victim `distance`
/// yards ahead down +Z.
fn spawn_hunter(app: &mut App, distance: f32) -> Hunter {
    let victim = app
        .world_mut()
        .spawn((
            Transform::from_xyz(0.0, 1.0, distance),
            Combatant::new(1, 0, CharacterClass::Rogue),
        ))
        .id();
    let mut combatant = Combatant::new(0, 0, CharacterClass::Hunter);
    combatant.weapon_speed = BOW_SPEED;
    combatant.melee_damage = 20.0;
    combatant.melee_weapon_speed = MELEE_SPEED;
    combatant.offhand_damage = 10.0;
    combatant.offhand_weapon_speed = 1.5;
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
    let bow = spawn_socket(app, unit, body, WeaponKind::Bow, WeaponHand::Main);
    let blade = spawn_socket(app, unit, body, WeaponKind::Dagger, WeaponHand::Main);
    let off = spawn_socket(app, unit, body, WeaponKind::Dagger, WeaponHand::Off);
    Hunter {
        unit,
        victim,
        bow,
        blade,
        off,
    }
}

fn move_victim(app: &mut App, hunter: &Hunter, distance: f32) {
    app.world_mut()
        .get_mut::<Transform>(hunter.victim)
        .unwrap()
        .translation = Vec3::new(0.0, 1.0, distance);
}

fn shown(app: &App, socket: Entity) -> bool {
    *app.world().get::<Visibility>(socket).unwrap() != Visibility::Hidden
}

/// (bow, blade, off-hand dagger) shown.
fn shown_sets(app: &App, hunter: &Hunter) -> (bool, bool, bool) {
    (
        shown(app, hunter.bow),
        shown(app, hunter.blade),
        shown(app, hunter.off),
    )
}

const BOW_OUT: (bool, bool, bool) = (true, false, false);
const MELEE_OUT: (bool, bool, bool) = (false, true, true);

fn swing(app: &mut App, hunter: &Hunter, kind: AutoAttackKind, hand: WeaponHand) {
    app.world_mut().spawn(AutoAttackSwing {
        attacker: hunter.unit,
        target: hunter.victim,
        kind,
        is_crit: false,
        hand,
    });
}

/// How far a socket's world pose is from rest: its rotation away from
/// identity (radians) plus its displacement from the owner (yards). Both, since
/// the bow's release is a forward SNAP with no rotation and a blade's is a
/// rotation-led stroke. The sockets rest at identity under an unrotated owner
/// at (0, 1, 0) facing its victim straight down +Z.
fn pose_angle(app: &App, socket: Entity) -> f32 {
    let pose = app
        .world()
        .get::<GlobalTransform>(socket)
        .unwrap()
        .compute_transform();
    pose.rotation.angle_between(Quat::IDENTITY) + (pose.translation - Vec3::Y).length()
}

/// The furthest the bow and the blade swing from rest over `FRAMES` frames.
fn max_poses(app: &mut App, hunter: &Hunter) -> (f32, f32) {
    let (mut bow, mut blade) = (0.0f32, 0.0f32);
    for _ in 0..FRAMES {
        app.update();
        bow = bow.max(pose_angle(app, hunter.bow));
        blade = blade.max(pose_angle(app, hunter.blade));
    }
    (bow, blade)
}

fn dead_zone() -> f32 {
    (MELEE_RANGE + HUNTER_DEAD_ZONE) / 2.0
}

/// At range the bow is out; in melee the hand weapons are, both of them.
#[test]
fn the_set_out_follows_the_targets_distance() {
    let mut app = harness();
    let hunter = spawn_hunter(&mut app, 20.0);
    app.update();
    assert_eq!(shown_sets(&app, &hunter), BOW_OUT, "at 20yd");

    move_victim(&mut app, &hunter, 1.0);
    app.update();
    assert_eq!(shown_sets(&app, &hunter), MELEE_OUT, "at 1yd");

    move_victim(&mut app, &hunter, HUNTER_DEAD_ZONE);
    app.update();
    assert_eq!(
        shown_sets(&app, &hunter),
        BOW_OUT,
        "at the Auto Shot minimum"
    );
}

/// Inside the dead zone the set does not change, whichever way the target
/// came in: that gap is what keeps a target hovering at an edge from
/// strobing the swap.
#[test]
fn the_dead_zone_keeps_whichever_set_was_out() {
    let mut app = harness();
    let hunter = spawn_hunter(&mut app, 20.0);
    app.update();
    move_victim(&mut app, &hunter, dead_zone());
    for _ in 0..FRAMES {
        app.update();
        assert_eq!(shown_sets(&app, &hunter), BOW_OUT, "closing from range");
    }

    move_victim(&mut app, &hunter, 1.0);
    app.update();
    move_victim(&mut app, &hunter, dead_zone());
    for _ in 0..FRAMES {
        app.update();
        assert_eq!(shown_sets(&app, &hunter), MELEE_OUT, "backing out of melee");
    }
}

/// A landed melee swing strikes with the blade, and the bow — the other
/// main-hand model — stays at rest.
#[test]
fn a_melee_swing_strikes_with_the_blade_not_the_bow() {
    let mut app = harness();
    let hunter = spawn_hunter(&mut app, 1.0);
    swing(&mut app, &hunter, AutoAttackKind::Melee, WeaponHand::Main);
    let (bow, blade) = max_poses(&mut app, &hunter);
    assert!(blade > MOVED, "the blade must strike: {blade} rad");
    assert!(bow < MOVED, "the bow must stay at rest: {bow} rad");
}

/// A landed Shot looses the bow, and the blade stays at rest.
#[test]
fn a_shot_looses_the_bow_not_the_blade() {
    let mut app = harness();
    let hunter = spawn_hunter(&mut app, 20.0);
    swing(&mut app, &hunter, AutoAttackKind::Shot, WeaponHand::Main);
    let (bow, blade) = max_poses(&mut app, &hunter);
    assert!(bow > MOVED, "the bow must loose: {bow} rad");
    assert!(blade < MOVED, "the blade must stay at rest: {blade} rad");
}

/// The blade telegraphs off the MELEE timer: with the melee swing about to
/// land and the bow's far from it, the blade winds up.
#[test]
fn the_blade_winds_up_on_the_melee_timer() {
    let mut app = harness();
    let hunter = spawn_hunter(&mut app, 1.0);
    {
        let mut c = app.world_mut().get_mut::<Combatant>(hunter.unit).unwrap();
        c.attack_timer = 0.0;
        c.melee_timer = MELEE_SPEED - 0.01;
    }
    let (_, blade) = max_poses(&mut app, &hunter);
    assert!(blade > MOVED, "the blade must wind up: {blade} rad");
}

/// And the converse: a bow shot about to fire does not raise the blade, whose
/// own timer is far from its swing.
#[test]
fn the_blade_ignores_the_bow_timer() {
    let mut app = harness();
    let hunter = spawn_hunter(&mut app, 1.0);
    {
        let mut c = app.world_mut().get_mut::<Combatant>(hunter.unit).unwrap();
        c.attack_timer = BOW_SPEED - 0.01;
        c.melee_timer = 0.0;
    }
    let (_, blade) = max_poses(&mut app, &hunter);
    assert!(blade < MOVED, "the blade must stay at rest: {blade} rad");
}
