//! Probes for the auto-attack stroke scaling with the weapon swinging it
//! (`consume_swing_signals` / `animate_weapon_swings` in
//! `rendering/effects/weapon_swing.rs`, and Heroic Strike's flourish in
//! `heroic_strike.rs`).
//!
//! The unit tests beside `weapon_stroke_profile` pin the FORMULA. These pin the
//! WIRING: that the stroke a socket actually plays is timed off its own hand's
//! interval, captured at the hit — so a dual-wielder's two weapons stroke at
//! their own lengths, and a slow landing mid-stroke re-times the next swing
//! rather than the one already playing.
//!
//! Every length is read off what reaches the screen — how long the socket's
//! `GlobalTransform` stays away from rest, when the landing's first particle
//! appears, when the trail is gone — never off a stored field. The expected
//! values are the signed-off AS-148 timings, written out rather than recomputed
//! through the code under test.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, AutoAttackKind, AutoAttackSwing, ClientParticle, Combatant,
    DispelType, HeroicStrikeSwing, SwingStyle, VisualBody, WeaponHand, WeaponKind, WeaponSocket,
};
use arenasim::states::play_match::{
    animate_client_particles, animate_school_impacts, animate_weapon_swings, cleanup_mortal_strike,
    consume_swing_signals, spawn_heroic_strike_flourish, spawn_school_impacts,
    update_weapon_trails, WeaponTrail,
};
use arenasim::CharacterClass;

/// Fine enough that a stroke length is resolved to 5ms.
const TICK: Duration = Duration::from_millis(5);
const DT: f32 = 0.005;
/// A pose this far from rest (radians) is a stroke, not float noise.
const MOVED: f32 = 1e-4;

// Signed-off stroke lengths — release + impact hold + follow-through.
/// Arcanite Reaper, 3.8s: 0.16 + 0.095 + 0.57.
const STROKE_3_8: f32 = 0.825;
/// Serpent Fang Dagger, 1.7s: 0.10 + 0.0425 + 0.255.
const STROKE_1_7: f32 = 0.3975;
/// Claw of Chromaggus, 1.5s: 0.10 + 0.04 + 0.225.
const STROKE_1_5: f32 = 0.365;
/// The 1.7s dagger under a 50% attack-speed slow swings at 3.4s:
/// 0.153 + 0.085 + 0.51.
const STROKE_1_7_SLOWED: f32 = 0.748;
/// Every phase at its floor — what a stroke timed off no interval plays.
const STROKE_FLOOR: f32 = 0.36;
/// The 3.8s release: the landing waits this long for the blade.
const RELEASE_3_8: f32 = 0.16;
/// The floor release.
const RELEASE_FLOOR: f32 = 0.10;
/// Heroic Strike's trail lingers this long after the stroke stops sampling.
const TRAIL_LIFETIME: f32 = 0.60;

fn swing_app() -> App {
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
    // The first update of a MinimalPlugins app advances no time; spend it
    // here so every stroke below starts on a ticking clock.
    app.update();
    app
}

struct Wielder {
    unit: Entity,
    target: Entity,
    main: Entity,
    off: Entity,
}

/// A melee unit at the origin facing its victim straight down +Z, holding a
/// weapon in each hand. The sim timers stay at 0 — nothing here ticks them —
/// so no windup ever starts and every departure from rest is the stroke.
fn spawn_wielder(app: &mut App, main_speed: f32, off_speed: Option<f32>) -> Wielder {
    let target = app
        .world_mut()
        .spawn((
            Transform::from_xyz(0.0, 0.0, 2.0),
            Combatant::new(1, 0, CharacterClass::Priest),
        ))
        .id();
    let mut combatant = Combatant::new(0, 0, CharacterClass::Warrior);
    combatant.stealthed = false;
    combatant.weapon_speed = main_speed;
    if let Some(speed) = off_speed {
        combatant.offhand_damage = 10.0;
        combatant.offhand_weapon_speed = speed;
    }
    combatant.target = Some(target);
    let unit = app
        .world_mut()
        .spawn((Transform::default(), combatant))
        .id();
    let body = app
        .world_mut()
        .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
        .id();
    app.world_mut().entity_mut(unit).add_child(body);
    let main = spawn_socket(app, unit, body, WeaponHand::Main);
    let off = spawn_socket(app, unit, body, WeaponHand::Off);
    Wielder {
        unit,
        target,
        main,
        off,
    }
}

fn spawn_socket(app: &mut App, owner: Entity, body: Entity, hand: WeaponHand) -> Entity {
    let socket = app
        .world_mut()
        .spawn((
            WeaponSocket {
                kind: WeaponKind::Mace,
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
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    app.world_mut().entity_mut(body).add_child(socket);
    socket
}

fn land(app: &mut App, w: &Wielder, hand: WeaponHand) {
    app.world_mut().spawn(AutoAttackSwing {
        attacker: w.unit,
        target: w.target,
        kind: AutoAttackKind::Melee,
        is_crit: false,
        hand,
    });
}

fn now(app: &App) -> f32 {
    app.world().resource::<Time>().elapsed_secs()
}

fn displaced(app: &App, socket: Entity) -> bool {
    let pose = app
        .world()
        .get::<GlobalTransform>(socket)
        .unwrap()
        .compute_transform();
    pose.rotation.angle_between(Quat::IDENTITY) > MOVED || pose.translation.length() > MOVED
}

/// Seconds from `start` to the last frame each socket was away from rest,
/// running `secs` of frames and calling `mid` once, `at` seconds in.
fn stroke_ends(
    app: &mut App,
    sockets: &[Entity],
    start: f32,
    secs: f32,
    mid: Option<(f32, &dyn Fn(&mut App))>,
) -> Vec<f32> {
    let mut ends = vec![0.0; sockets.len()];
    let mut mid = mid;
    while now(app) - start < secs {
        if let Some((at, f)) = mid {
            if now(app) - start >= at {
                f(app);
                mid = None;
            }
        }
        app.update();
        for (end, socket) in ends.iter_mut().zip(sockets) {
            if displaced(app, *socket) {
                *end = now(app) - start;
            }
        }
    }
    ends
}

/// The stroke ended within one tick of `expected` — its last displaced frame
/// falls in the tick before it expires.
fn assert_stroke(label: &str, measured: f32, expected: f32) {
    assert!(
        measured > expected - 2.0 * DT && measured <= expected + DT,
        "{label}: the stroke lasted {measured}s, expected {expected}s"
    );
}

/// The stroke a landed swing plays lasts the signed-off length for its
/// weapon's speed — the Reaper's heavy swing and the dagger's quick one —
/// not the floor every phase clamps to.
#[test]
fn a_landed_stroke_lasts_as_long_as_its_weapon_swings() {
    for (speed, expected) in [(3.8, STROKE_3_8), (1.7, STROKE_1_7)] {
        let mut app = swing_app();
        let w = spawn_wielder(&mut app, speed, None);
        let start = now(&app);
        land(&mut app, &w, WeaponHand::Main);
        let ends = stroke_ends(&mut app, &[w.main], start, 1.5, None);
        assert_stroke(&format!("{speed}s weapon"), ends[0], expected);
        assert!(
            (ends[0] - STROKE_FLOOR).abs() > 0.03,
            "{speed}s weapon played the floor stroke"
        );
    }
}

/// A dual-wielder's two weapons each stroke at their OWN hand's length: a
/// 3.8s main hand and a 1.5s off hand landing on the same tick finish ~0.46s
/// apart.
#[test]
fn each_hand_strokes_at_its_own_weapons_length() {
    let mut app = swing_app();
    let w = spawn_wielder(&mut app, 3.8, Some(1.5));
    let start = now(&app);
    land(&mut app, &w, WeaponHand::Main);
    land(&mut app, &w, WeaponHand::Off);
    let ends = stroke_ends(&mut app, &[w.main, w.off], start, 1.5, None);
    assert_stroke("main hand (3.8s)", ends[0], STROKE_3_8);
    assert_stroke("off hand (1.5s)", ends[1], STROKE_1_5);
}

fn attack_speed_slow(magnitude: f32) -> Aura {
    Aura {
        effect_type: AuraType::AttackSpeedSlow,
        duration: 30.0,
        magnitude,
        break_on_damage_threshold: -1.0,
        accumulated_damage: 0.0,
        tick_interval: 0.0,
        time_until_next_tick: 0.0,
        caster: None,
        ability_name: "Thunder Clap".to_string(),
        fear_direction: (0.0, 0.0),
        fear_direction_timer: 0.0,
        spell_school: None,
        applied_this_frame: false,
        backlash_damage: None,
        dr_category_override: None,
        dispel_type: DispelType::Auto,
        compound: None,
        source_item: None,
        stacks: None,
        bloom: None,
    }
}

/// A slow that lands mid-stroke leaves the stroke already playing at the
/// length it began with; the NEXT landed swing plays the slowed length.
#[test]
fn a_slow_mid_stroke_retimes_the_next_swing_not_this_one() {
    let mut app = swing_app();
    let w = spawn_wielder(&mut app, 1.7, None);
    let start = now(&app);
    land(&mut app, &w, WeaponHand::Main);
    let unit = w.unit;
    let slow = move |app: &mut App| {
        app.world_mut().entity_mut(unit).insert(ActiveAuras {
            auras: vec![attack_speed_slow(0.5)],
        });
    };
    let ends = stroke_ends(&mut app, &[w.main], start, 1.0, Some((0.1, &slow)));
    assert_stroke("the stroke the slow landed in", ends[0], STROKE_1_7);

    let start = now(&app);
    land(&mut app, &w, WeaponHand::Main);
    let ends = stroke_ends(&mut app, &[w.main], start, 1.2, None);
    assert_stroke("the next, slowed stroke", ends[0], STROKE_1_7_SLOWED);
}

// --- Heroic Strike ------------------------------------------------------------

fn heroic_app() -> App {
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
    app.insert_resource(AbilityDefinitions::default());
    app.add_systems(
        Update,
        (
            spawn_heroic_strike_flourish,
            retire_markers,
            spawn_school_impacts,
            animate_school_impacts,
            animate_client_particles,
            update_weapon_trails,
            cleanup_mortal_strike,
        )
            .chain(),
    );
    app.update();
    app
}

/// Stands in for `consume_swing_signals`, the marker's last consumer.
fn retire_markers(mut commands: Commands, markers: Query<Entity, With<AutoAttackSwing>>) {
    for e in markers.iter() {
        commands.entity(e).despawn();
    }
}

fn live_particles(app: &mut App) -> usize {
    let mut q = app.world_mut().query::<&ClientParticle>();
    q.iter(app.world()).filter(|p| p.age < p.life).count()
}

fn trails(app: &mut App) -> usize {
    let mut q = app
        .world_mut()
        .query_filtered::<Entity, With<WeaponTrail>>();
    q.iter(app.world()).count()
}

/// Heroic Strike rides the ordinary auto stroke, so on a 3.8s two-hander its
/// landing waits for the heavy swing's release and its trail runs for that
/// swing's whole length — not the floor stroke's.
#[test]
fn heroic_strike_follows_the_heavy_weapons_stroke() {
    let mut app = heroic_app();
    let target = app
        .world_mut()
        .spawn((
            Combatant::new(1, 0, CharacterClass::Mage),
            Transform::from_translation(Vec3::ZERO),
        ))
        .id();
    let mut warrior = Combatant::new(0, 0, CharacterClass::Warrior);
    warrior.weapon_speed = 3.8;
    let attacker = app
        .world_mut()
        .spawn((warrior, Transform::from_xyz(-2.0, 0.0, 0.0)))
        .id();
    let start = now(&app);
    app.world_mut().spawn((
        AutoAttackSwing {
            attacker,
            target,
            kind: AutoAttackKind::Melee,
            is_crit: false,
            hand: WeaponHand::Main,
        },
        HeroicStrikeSwing,
    ));

    let mut first_particle = None;
    let mut trail_gone = None;
    while now(&app) - start < 2.0 {
        app.update();
        let t = now(&app) - start;
        if first_particle.is_none() && live_particles(&mut app) > 0 {
            first_particle = Some(t);
        }
        if trail_gone.is_none() && t > DT && trails(&mut app) == 0 {
            trail_gone = Some(t);
        }
    }
    let landed = first_particle.expect("the empowered swing never landed");
    assert!(
        landed >= RELEASE_3_8 - DT && landed <= RELEASE_3_8 + 3.0 * DT,
        "the landing fired {landed}s in; the 3.8s blade arrives at {RELEASE_3_8}s \
         (the floor stroke's at {RELEASE_FLOOR}s)"
    );
    let gone = trail_gone.expect("the trail never cleared");
    let expected = STROKE_3_8 + TRAIL_LIFETIME;
    assert!(
        (gone - expected).abs() <= 2.0 * DT,
        "the trail cleared {gone}s in; the 3.8s stroke plus its tail is {expected}s \
         (the floor stroke's would be {}s)",
        STROKE_FLOOR + TRAIL_LIFETIME
    );
}
