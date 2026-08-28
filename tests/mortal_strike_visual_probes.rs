//! Probes for the Mortal Strike signature (`instant_ability.rs`,
//! `mortal_strike.rs`, `mortal_wounds.rs`).
//!
//! Appearance is not testable here. What is — and what would fail silently —
//! is the CONTRACT between the sim's cosmetic markers and the graphical
//! systems that consume them:
//!
//! * every marker is consumed and despawned, signature or not, so headless
//!   never leaks marker entities and the graphical client never double-fires;
//! * the styled stroke lands on the right socket and is ONE-SHOT, so a
//!   signature's timing can never bleed into the following auto-attack;
//! * the heal fracture keys on the reduction, so Aimed Shot inherits it and an
//!   unafflicted heal produces nothing.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` — no window, no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::combat_core::refused_fraction;
use arenasim::states::play_match::components::{
    Combatant, DeathAnimation, HealingRefused, InstantAbilityFired, SwingStyle, VisualBody,
    WeaponHand, WeaponKind, WeaponSocket,
};
use arenasim::states::play_match::{
    animate_body_lean, cleanup_heal_fracture, cleanup_mortal_strike, consume_instant_ability_signals,
    consume_swing_signals, spawn_heal_fracture, update_heal_fracture,
    update_mortal_strike_flash, update_mortal_strike_impacts, update_mortal_strike_sparks,
    update_mortal_strike_trail, MortalStrikePendingImpact, MortalStrikeSpark, RefusedHealMote,
};
use arenasim::states::play_match::components::AutoAttackSwing;
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(50);

fn harness() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    // The gesture router builds the rogue crescent texture on demand, so it
    // needs an `Assets<Image>` even for abilities that never spawn one.
    app.init_asset::<Image>();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
    app
}

/// A combatant carrying one main-hand two-hand axe socket, in the same
/// hierarchy `spawn_combatant` builds for a Warrior: the sim entity owns a
/// `VisualBody` child, and the socket hangs off that body.
fn spawn_warrior(app: &mut App) -> (Entity, Entity) {
    let attacker = app
        .world_mut()
        .spawn((
            Transform::from_xyz(0.0, 0.0, 0.0),
            Combatant::new(0, 0, CharacterClass::Warrior),
        ))
        .id();
    let body = app
        .world_mut()
        .spawn((VisualBody { rest_y: 1.0 }, Transform::from_xyz(0.0, 1.0, 0.0)))
        .id();
    app.world_mut().entity_mut(attacker).add_child(body);
    let socket = app
        .world_mut()
        .spawn((
            WeaponSocket {
                kind: WeaponKind::TwoHandAxe,
                hand: WeaponHand::Main,
                owner: attacker,
                rest: Transform::IDENTITY,
                release_t: None,
                aim: Vec3::ZERO,
                winds_up_next: true,
                yaw_local: 0.0,
                prev_owner_yaw: 0.0,
                windup_s: 0.0,
                swing_style: SwingStyle::Auto,
                last_s: 0.0,
            },
            Transform::IDENTITY,
            GlobalTransform::default(),
            Visibility::default(),
        ))
        .id();
    app.world_mut().entity_mut(body).add_child(socket);
    (attacker, socket)
}

/// The `VisualBody` child of a combatant built by [`spawn_warrior`].
fn body_of(app: &mut App, attacker: Entity) -> Entity {
    let children = app.world().entity(attacker).get::<Children>().unwrap();
    let candidates: Vec<Entity> = children.iter().collect();
    candidates
        .into_iter()
        .find(|e| app.world().entity(*e).get::<VisualBody>().is_some())
        .expect("the warrior has a VisualBody child")
}

fn spawn_target(app: &mut App, x: f32) -> Entity {
    app.world_mut()
        .spawn((
            Transform::from_xyz(x, 0.0, 0.0),
            Combatant::new(1, 0, CharacterClass::Priest),
        ))
        .id()
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

#[test]
fn mortal_strike_starts_the_styled_stroke_on_the_main_hand() {
    let mut app = harness();
    app.add_systems(Update, consume_instant_ability_signals);
    let (attacker, socket) = spawn_warrior(&mut app);
    let target = spawn_target(&mut app, 2.0);

    let marker = app
        .world_mut()
        .spawn(InstantAbilityFired {
            caster: attacker,
            target: Some(target),
            ability: AbilityType::MortalStrike,
            is_crit: false,
        })
        .id();

    app.update();

    let socket_state = app.world().entity(socket).get::<WeaponSocket>().unwrap();
    assert_eq!(socket_state.swing_style, SwingStyle::MortalStrike);
    assert_eq!(socket_state.release_t, Some(0.0));
    assert_eq!(
        socket_state.windup_s, -1.0,
        "the signature forces a full windup so the arc is the same size every cast"
    );
    assert!(
        app.world().get_entity(marker).is_err(),
        "the marker must be consumed"
    );
}

#[test]
fn an_instant_without_a_signature_leaves_the_socket_alone_but_still_consumes_its_marker() {
    let mut app = harness();
    app.add_systems(Update, consume_instant_ability_signals);
    let (attacker, socket) = spawn_warrior(&mut app);
    let target = spawn_target(&mut app, 2.0);

    // Rogue instants drain through the same loop and get the same marker; they
    // simply have no signature yet.
    let marker = app
        .world_mut()
        .spawn(InstantAbilityFired {
            caster: attacker,
            target: Some(target),
            ability: AbilityType::SinisterStrike,
            is_crit: false,
        })
        .id();

    app.update();

    let socket_state = app.world().entity(socket).get::<WeaponSocket>().unwrap();
    assert_eq!(socket_state.swing_style, SwingStyle::Auto);
    assert_eq!(socket_state.release_t, None, "no stroke was started");
    assert!(
        app.world().get_entity(marker).is_err(),
        "markers for un-signatured abilities must still be despawned, or they leak"
    );
}

#[test]
fn an_ordinary_auto_clears_a_signature_style() {
    // The regression that motivates the reset in `consume_swing_signals`:
    // without it, an auto landing while a Mortal Strike stroke is still
    // playing inherits the signature's slower timing and deeper arc.
    let mut app = harness();
    app.add_systems(Update, (consume_swing_signals, consume_instant_ability_signals).chain());
    let (attacker, socket) = spawn_warrior(&mut app);
    let target = spawn_target(&mut app, 2.0);

    app.world_mut().spawn(InstantAbilityFired {
        caster: attacker,
        target: Some(target),
        ability: AbilityType::MortalStrike,
        is_crit: false,
    });
    app.update();
    assert_eq!(
        app.world().entity(socket).get::<WeaponSocket>().unwrap().swing_style,
        SwingStyle::MortalStrike
    );

    app.world_mut().spawn(AutoAttackSwing { attacker, target, ranged: false });
    app.update();

    assert_eq!(
        app.world().entity(socket).get::<WeaponSocket>().unwrap().swing_style,
        SwingStyle::Auto,
        "an auto-attack must not inherit the signature stroke"
    );
}

#[test]
fn a_same_tick_auto_does_not_downgrade_the_signature() {
    // Both land on one tick. The registration orders the instant consumer AFTER
    // the auto consumer precisely so the special wins the socket.
    let mut app = harness();
    app.add_systems(Update, (consume_swing_signals, consume_instant_ability_signals).chain());
    let (attacker, socket) = spawn_warrior(&mut app);
    let target = spawn_target(&mut app, 2.0);

    app.world_mut().spawn(AutoAttackSwing { attacker, target, ranged: false });
    app.world_mut().spawn(InstantAbilityFired {
        caster: attacker,
        target: Some(target),
        ability: AbilityType::MortalStrike,
        is_crit: false,
    });
    app.update();

    assert_eq!(
        app.world().entity(socket).get::<WeaponSocket>().unwrap().swing_style,
        SwingStyle::MortalStrike,
        "the signature must win a same-tick race with an ordinary auto"
    );
}

#[test]
fn the_impact_waits_for_the_blade_to_arrive() {
    // The sim resolves an instant BEFORE the animation plays, so the stroke
    // starts at the hit. Spawning the burst there puts it on screen with the
    // weapon still wound up, and it expires before the blade travels.
    let mut app = harness();
    app.add_systems(
        Update,
        (consume_instant_ability_signals, update_mortal_strike_impacts).chain(),
    );
    let (attacker, _socket) = spawn_warrior(&mut app);
    let target = spawn_target(&mut app, 2.0);

    app.world_mut().spawn(InstantAbilityFired {
        caster: attacker,
        target: Some(target),
        ability: AbilityType::MortalStrike,
        is_crit: false,
    });

    let sparks = |app: &mut App| {
        app.world_mut().query::<&MortalStrikeSpark>().iter(app.world()).count()
    };

    app.update();
    assert_eq!(sparks(&mut app), 0, "no burst while the blade is still wound up");

    // The stroke's contact frame is `SwingStyle::MortalStrike.impact_at()`;
    // step past it and the burst must appear exactly once.
    let contact_ticks = (SwingStyle::MortalStrike.impact_at() / TICK.as_secs_f32()).ceil() as u32;
    for _ in 0..=contact_ticks {
        app.update();
    }
    let fired = sparks(&mut app);
    assert!(fired > 0, "the burst must fire when the blade arrives");

    app.update();
    assert!(
        app.world_mut()
            .query::<&MortalStrikePendingImpact>()
            .iter(app.world())
            .next()
            .is_none(),
        "the pending impact must be consumed, not left to re-fire"
    );
}

#[test]
fn the_flourish_expires_without_leaking_entities() {
    let mut app = harness();
    app.add_systems(
        Update,
        (
            consume_instant_ability_signals,
            update_mortal_strike_trail,
            update_mortal_strike_impacts,
            update_mortal_strike_flash,
            update_mortal_strike_sparks,
            cleanup_mortal_strike,
        )
            .chain(),
    );
    let (attacker, _socket) = spawn_warrior(&mut app);
    let target = spawn_target(&mut app, 2.0);

    app.world_mut().spawn(InstantAbilityFired {
        caster: attacker,
        target: Some(target),
        ability: AbilityType::MortalStrike,
        is_crit: true,
    });

    // Run past the contact frame so the burst actually exists, then well past
    // every lifetime. Checking only the first tick would miss the delayed
    // entities entirely.
    for _ in 0..12 {
        app.update();
    }
    let mid = app.world_mut().query::<&MortalStrikeSpark>().iter(app.world()).count();
    assert!(mid > 0, "the burst fired before the sweep to cleanup");

    for _ in 0..60 {
        app.update();
    }

    let trails = app
        .world_mut()
        .query::<&arenasim::states::play_match::MortalStrikeTrail>()
        .iter(app.world())
        .count();
    let flashes = app
        .world_mut()
        .query::<&arenasim::states::play_match::MortalStrikeFlash>()
        .iter(app.world())
        .count();
    let sparks = app
        .world_mut()
        .query::<&arenasim::states::play_match::MortalStrikeSpark>()
        .iter(app.world())
        .count();
    assert_eq!((trails, flashes, sparks), (0, 0, 0), "the flourish must fully clean up");
}

// ---------------------------------------------------------------------------
// Body lean
// ---------------------------------------------------------------------------

#[test]
fn the_body_stands_upright_between_swings() {
    // The lean writes every frame, so a unit that is not swinging must be
    // written back to upright — otherwise the first swing of a match leaves
    // every combatant permanently tilted.
    let mut app = harness();
    app.add_systems(Update, animate_body_lean);
    let (attacker, _socket) = spawn_warrior(&mut app);
    let body = body_of(&mut app, attacker);

    app.update();

    let t = app.world().entity(body).get::<Transform>().unwrap();
    assert!(t.rotation.angle_between(Quat::IDENTITY) < 1e-5, "upright at rest");
    assert!(t.translation.z.abs() < 1e-5, "no step at rest");
}

#[test]
fn the_body_leans_while_the_weapon_swings() {
    let mut app = harness();
    app.add_systems(Update, animate_body_lean);
    let (attacker, socket) = spawn_warrior(&mut app);
    let body = body_of(&mut app, attacker);

    // Mid-release of a signature stroke.
    {
        let mut s = app.world_mut().entity_mut(socket);
        let mut socket_state = s.get_mut::<WeaponSocket>().unwrap();
        socket_state.swing_style = SwingStyle::MortalStrike;
        socket_state.last_s = 1.0;
    }
    app.update();

    let t = app.world().entity(body).get::<Transform>().unwrap();
    assert!(
        t.rotation.angle_between(Quat::IDENTITY) > 0.1,
        "the torso must visibly turn into a signature swing"
    );
    assert!(t.translation.z > 0.05, "and step forward into it");
}

#[test]
fn a_dying_unit_cedes_rotation_and_loses_its_step() {
    // The death fall owns the body's rotation. The lean must not fight it —
    // and must clear the horizontal step, which nothing else writes, or a unit
    // killed mid-swing keeps the offset on its corpse for the rest of the match.
    let mut app = harness();
    app.add_systems(Update, animate_body_lean);
    let (attacker, socket) = spawn_warrior(&mut app);
    let body = body_of(&mut app, attacker);

    {
        let mut s = app.world_mut().entity_mut(socket);
        let mut socket_state = s.get_mut::<WeaponSocket>().unwrap();
        socket_state.swing_style = SwingStyle::MortalStrike;
        socket_state.last_s = 1.0;
    }
    app.update();
    assert!(app.world().entity(body).get::<Transform>().unwrap().translation.z > 0.05);

    // The unit dies mid-swing; the death fall sets its own rotation.
    let death_rotation = Quat::from_rotation_x(1.0);
    app.world_mut().entity_mut(attacker).insert(DeathAnimation::new(Vec3::X));
    app.world_mut().entity_mut(body).get_mut::<Transform>().unwrap().rotation = death_rotation;
    app.update();

    let t = app.world().entity(body).get::<Transform>().unwrap();
    assert!(
        t.rotation.angle_between(death_rotation) < 1e-5,
        "the death fall keeps the rotation it set"
    );
    assert!(t.translation.z.abs() < 1e-5, "the step is cleared on death");
}

// ---------------------------------------------------------------------------
// Mortal Wounds heal fracture
// ---------------------------------------------------------------------------

#[test]
fn a_refused_heal_sheds_ash_and_consumes_its_marker() {
    let mut app = harness();
    app.add_systems(Update, spawn_heal_fracture);
    let target = spawn_target(&mut app, 0.0);

    let marker = app
        .world_mut()
        .spawn(HealingRefused { target, refused_fraction: 0.35 })
        .id();

    app.update();

    let motes = app.world_mut().query::<&RefusedHealMote>().iter(app.world()).count();
    assert!(motes > 0, "a refused heal must shed visible ash");
    assert!(app.world().get_entity(marker).is_err(), "the marker must be consumed");
}

#[test]
fn a_marker_for_a_despawned_target_is_still_consumed() {
    // A target can die on the same frame its heal was cut. The marker must not
    // survive the match as a leak.
    let mut app = harness();
    app.add_systems(Update, spawn_heal_fracture);
    let target = spawn_target(&mut app, 0.0);
    app.world_mut().entity_mut(target).despawn();

    let marker = app
        .world_mut()
        .spawn(HealingRefused { target, refused_fraction: 0.35 })
        .id();

    app.update();

    assert!(app.world().get_entity(marker).is_err(), "orphaned markers must be consumed");
    assert_eq!(
        app.world_mut().query::<&RefusedHealMote>().iter(app.world()).count(),
        0,
        "no ash without a target to shed it from"
    );
}

#[test]
fn ash_expires() {
    let mut app = harness();
    app.add_systems(
        Update,
        (spawn_heal_fracture, update_heal_fracture, cleanup_heal_fracture).chain(),
    );
    let target = spawn_target(&mut app, 0.0);
    app.world_mut().spawn(HealingRefused { target, refused_fraction: 0.8 });

    app.update();
    assert!(app.world_mut().query::<&RefusedHealMote>().iter(app.world()).count() > 0);

    for _ in 0..60 {
        app.update();
    }
    assert_eq!(
        app.world_mut().query::<&RefusedHealMote>().iter(app.world()).count(),
        0,
        "ash must self-expire"
    );
}

// ---------------------------------------------------------------------------
// The reduction seam
// ---------------------------------------------------------------------------

#[test]
fn an_unreduced_heal_spawns_no_marker() {
    assert_eq!(refused_fraction(100.0, 100.0), None);
}

#[test]
fn mortal_strikes_magnitude_reports_its_refused_share() {
    // 0.65 multiplier => 35% refused. This is the value both Mortal Strike and
    // Aimed Shot carry, which is why the tell is keyed on the reduction rather
    // than on either ability.
    let refused = refused_fraction(100.0, 65.0).expect("a cut heal reports a refusal");
    assert!((refused - 0.35).abs() < 1e-5, "expected 0.35, got {refused}");
}

#[test]
fn a_zero_heal_reports_nothing() {
    // Overheal / a heal that resolved to nothing must not divide by zero or
    // spawn a marker.
    assert_eq!(refused_fraction(0.0, 0.0), None);
}

#[test]
fn stacked_reductions_report_the_combined_share() {
    // Dampening stacks on top of the debuff late in a match; the tell should
    // still read.
    let refused = refused_fraction(100.0, 20.0).expect("a heavily cut heal reports a refusal");
    assert!((refused - 0.8).abs() < 1e-5, "expected 0.8, got {refused}");
}
