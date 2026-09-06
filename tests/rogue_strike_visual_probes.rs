//! Probes for the rogue strike gestures (Ambush, Sinister Strike).
//!
//! Both resolve through the `QueuedInstantAttack` drain and used to hit the
//! router's `_` fall-through — they rendered NOTHING. What these pin is the
//! world-space shape of each new stroke, sampled through the full animation
//! stack (`consume_instant_ability_signals` -> `animate_weapon_swings` ->
//! `animate_body_lean` -> transform propagation), not stored style fields: a
//! probe on `SwingArc` parameters would pass with the weapon pointing 90
//! degrees wrong (`test-geometry-not-bookkeeping`), and the socket's pose is
//! finalized by `animate_weapon_swings` each frame, so the harness must run it.
//!
//! Client-data grounding (wago.tools, 1.15.9.69547): Ambush shares Backstab's
//! visual wholesale — anim `Attack1HPierce` (85), 634ms cast model — so it is
//! a fast PIERCE, Kidney Shot's family at half its 1233ms. Sinister Strike is
//! plain `Attack1H` (17), Cheap Shot's anim, with no cast model — a quick
//! slash whose tilted plane is its distinction.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::components::{
    Combatant, InstantAbilityFired, SwingStyle, VisualBody, WeaponHand, WeaponKind, WeaponSocket,
};
use arenasim::states::play_match::{
    animate_body_lean, animate_weapon_swings, consume_instant_ability_signals, swing_plane_tilt,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(50);

/// Where the rogue stands; the socket's rest world position, since every
/// local transform under it is identity.
const ROGUE_POS: Vec3 = Vec3::new(0.0, 1.0, 0.0);

struct Harness {
    app: App,
    socket: Option<Entity>,
}

impl Harness {
    fn new() -> Self {
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
        // The full caster-side gesture stack, in registration order: the
        // router claims the stroke, the swing system writes the socket pose,
        // the lean system turns the body under it. Propagation (TransformPlugin,
        // PostUpdate) then finalizes the world-space transform we sample.
        app.add_systems(
            Update,
            (
                consume_instant_ability_signals,
                animate_weapon_swings,
                animate_body_lean,
            )
                .chain(),
        );
        Harness { app, socket: None }
    }

    /// A Rogue with a main-hand dagger socket under a visual body, mirroring
    /// the unit -> body -> socket hierarchy `class_weapon_loadout` builds.
    fn spawn_rogue(&mut self) -> Entity {
        let unit = self
            .app
            .world_mut()
            .spawn((
                Transform::from_translation(ROGUE_POS),
                Combatant::new(0, 0, CharacterClass::Rogue),
            ))
            .id();
        let body = self
            .app
            .world_mut()
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        let socket = self
            .app
            .world_mut()
            .spawn((
                WeaponSocket {
                    kind: WeaponKind::Dagger,
                    hand: WeaponHand::Main,
                    owner: unit,
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
                Transform::default(),
                Visibility::default(),
            ))
            .id();
        self.app.world_mut().entity_mut(body).add_child(socket);
        self.app.world_mut().entity_mut(unit).add_child(body);
        self.socket = Some(socket);
        unit
    }

    /// Victim 3yd straight ahead on +Z, so the aim axis is world Z.
    fn spawn_victim(&mut self) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, 1.0, 3.0),
                Combatant::new(1, 0, CharacterClass::Priest),
            ))
            .id()
    }

    fn fire(&mut self, caster: Entity, target: Entity, ability: AbilityType) {
        self.app.world_mut().spawn(InstantAbilityFired {
            caster,
            target: Some(target),
            ability,
            is_crit: false,
        });
    }

    /// Tick through the whole stroke, sampling the socket's WORLD transform
    /// after each frame: (displacement from rest, blade direction).
    fn sample_stroke(&mut self, ticks: u32) -> Vec<(Vec3, Vec3)> {
        let socket = self.socket.expect("spawn_rogue first");
        let mut samples = Vec::new();
        for _ in 0..ticks {
            self.app.update();
            let gt = self
                .app
                .world()
                .get::<GlobalTransform>(socket)
                .expect("socket has a GlobalTransform");
            let (_, rot, pos) = gt.to_scale_rotation_translation();
            samples.push((pos - ROGUE_POS, rot * Vec3::Z));
        }
        samples
    }

    fn style(&mut self) -> SwingStyle {
        self.app
            .world()
            .get::<WeaponSocket>(self.socket.expect("spawn_rogue first"))
            .unwrap()
            .swing_style
    }

    fn markers(&mut self) -> usize {
        self.app
            .world_mut()
            .query::<&InstantAbilityFired>()
            .iter(self.app.world())
            .count()
    }
}

#[test]
fn each_strike_claims_its_own_stroke() {
    // The whole card: both markers used to fall through the router and render
    // nothing. If either arm regresses to `_`, the stroke stays `Auto`.
    for (ability, style) in [
        (AbilityType::Ambush, SwingStyle::Ambush),
        (AbilityType::SinisterStrike, SwingStyle::SinisterStrike),
    ] {
        let mut h = Harness::new();
        let rogue = h.spawn_rogue();
        let victim = h.spawn_victim();
        h.fire(rogue, victim, ability);
        h.sample_stroke(1);
        assert_eq!(h.style(), style, "{ability:?} must claim its own stroke");
        assert_eq!(h.markers(), 0, "the marker is consumed as it is read");
    }
}

#[test]
fn ambush_drives_the_dagger_through_the_target_in_world_space() {
    // The source is `Attack1HPierce`: a pierce is TRANSLATION. The weapon must
    // visibly load back and then drive forward along the aim axis (+Z here,
    // victim straight ahead), well past the dagger auto's 0.85yd lunge —
    // otherwise the opener reads as a routine stab.
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::Ambush);
    // 15 ticks x 50ms covers the ~0.64s stroke with follow-through.
    let samples = h.sample_stroke(15);

    let min_z = samples.iter().map(|(d, _)| d.z).fold(f32::INFINITY, f32::min);
    let max_z = samples
        .iter()
        .map(|(d, _)| d.z)
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        min_z < -0.25,
        "the pierce never loaded back — min world-Z displacement {min_z:.2}yd"
    );
    assert!(
        max_z > 1.3,
        "the pierce reached only {max_z:.2}yd forward — the dagger auto \
         already lunges 0.85yd, so this would read as a routine stab"
    );
}

#[test]
fn ambush_is_a_pierce_the_blade_never_turns_over() {
    // What separates a pierce from a slash, stated in world space: the blade
    // keeps pointing at the victim throughout. A slash sweeps the blade
    // direction through more than a radian; a thrust barely rotates it.
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::Ambush);
    let samples = h.sample_stroke(15);

    for (i, (_, dir)) in samples.iter().enumerate() {
        let off = dir.angle_between(Vec3::Z);
        assert!(
            off < 0.5,
            "at sample {i} the blade points {off:.2} rad off the aim axis — \
             that is a swing, not a pierce"
        );
    }
}

#[test]
fn sinister_strike_sweeps_a_slash_not_a_thrust() {
    // The source is plain `Attack1H`: a swing is ROTATION. The blade direction
    // must sweep through a large world-space angle over the stroke while the
    // weapon itself goes nowhere — the inverse of Ambush's shape.
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::SinisterStrike);
    let samples = h.sample_stroke(14);

    let mut max_sweep = 0.0_f32;
    for (_, a) in &samples {
        for (_, b) in &samples {
            max_sweep = max_sweep.max(a.angle_between(*b));
        }
    }
    assert!(
        max_sweep > 1.5,
        "the blade swept only {max_sweep:.2} rad — that is a poke, not a slash"
    );

    let max_disp = samples
        .iter()
        .map(|(d, _)| d.length())
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        max_disp < 0.6,
        "the weapon travelled {max_disp:.2}yd — Sinister Strike is a swing, \
         and a thrust that far is Ambush's grammar"
    );
}

#[test]
fn sinister_strikes_plane_is_visibly_tilted_off_cheap_shots() {
    // The two share their source anim (`Attack1H`), and Cheap Shot's
    // distinction lives in a crescent flare this deliberately lacks — so the
    // PLANE is all that keeps them apart. In world space: on a 0.55 tilt the
    // blade direction picks up a lateral (X) component past 0.3 during the
    // sweep, which Cheap Shot's 0.25 tilt cannot reach (its ceiling is ~0.22).
    let mut h = Harness::new();
    let rogue = h.spawn_rogue();
    let victim = h.spawn_victim();
    h.fire(rogue, victim, AbilityType::SinisterStrike);
    let samples = h.sample_stroke(14);

    let max_x = samples
        .iter()
        .map(|(_, dir)| dir.x.abs())
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        max_x > 0.3,
        "the blade's lateral reach is {max_x:.2} — inside Cheap Shot's plane, \
         so the two jabs would read as the same stroke"
    );

    // And the style-level statement of the same fact, for the tuning consts.
    let ss = swing_plane_tilt(SwingStyle::SinisterStrike).expect("SS traces a plane");
    let cheap = swing_plane_tilt(SwingStyle::CheapShot).expect("CS traces a plane");
    assert!(
        (ss - cheap).abs() > 0.15,
        "tilts {ss:.2} vs {cheap:.2} — the planes have converged"
    );
    // Ambush traces no plane at all: it is a lunge.
    assert!(swing_plane_tilt(SwingStyle::Ambush).is_none());
}

#[test]
fn ambush_is_the_quick_pierce_kidney_shot_the_slow_one() {
    // The source: backstab_cast_base.m2 runs 634ms — byte-for-byte the length
    // of sap_cast_base.m2 (Cheap Shot's clock) and HALF Kidney Shot's 1233ms.
    // Pace is the whole separation between the two lunges; if Ambush ever
    // slows toward Kidney Shot, the opener and the finisher collapse into one
    // gesture.
    let ambush = SwingStyle::Ambush.stroke_secs();
    let kidney = SwingStyle::KidneyShot.stroke_secs();
    let cheap = SwingStyle::CheapShot.stroke_secs();
    assert!(
        kidney > ambush * 1.5,
        "kidney {kidney:.2}s must stay well past ambush {ambush:.2}s"
    );
    assert!(
        (ambush - cheap).abs() < 0.05,
        "ambush {ambush:.2}s should sit in Cheap Shot's 634ms class \
         ({cheap:.2}s)"
    );
}

#[test]
fn the_builder_stays_quick_but_never_undercuts_the_interrupts() {
    // Sinister Strike fires every couple of GCDs — it must stay in the
    // quick-jab class (at or under Cheap Shot) without stealing the
    // interrupts' reflex slot: Pummel and Kick are the most urgent buttons in
    // the game and stay the fastest gestures.
    let ss = SwingStyle::SinisterStrike.stroke_secs();
    let cheap = SwingStyle::CheapShot.stroke_secs();
    let pummel = SwingStyle::Pummel.stroke_secs();
    let kick = SwingStyle::Kick.stroke_secs();
    assert!(ss <= cheap, "SS {ss:.2}s reads as ceremony past {cheap:.2}s");
    assert!(
        ss > pummel && ss > kick,
        "SS {ss:.2}s undercuts an interrupt ({pummel:.2}s / {kick:.2}s)"
    );
}
