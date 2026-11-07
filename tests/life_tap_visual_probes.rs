//! Probes for Life Tap's landing (`rendering/effects/life_tap.rs`, AS-340):
//! `lifetap.m2` on the Warlock's own chest.
//!
//! Everything is read off the rendered world — `GlobalTransform` and the live
//! material colour — never the rig's stored fields: where the glow sits, how
//! big it is and how it throbs, when it peaks and when it is gone, that it
//! faces the camera and rides the Warlock and goes with it, that the red
//! motes close on the chest and turn blue, that the green puff bursts and
//! stalls, that the cyan twinkles wait for their gate and spin, and that every
//! layer's particles render at the bench's size.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` with the
//! production systems in production order: the instant-ability router (which
//! production runs in `FixedUpdate`, ahead of every `Update` system), the
//! landing's two systems, then the shared particle ageing and billboarding.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{
    ClientParticle, Combatant, InstantAbilityFired, LifeTapGlow, LifeTapGlowQuad,
};
use arenasim::states::play_match::{
    animate_client_particles, billboard_hunter_shots, consume_instant_ability_signals,
    spawn_life_tap_glow_visuals, update_life_tap_glows, IMPACT_CHEST_Y, LIFE_TAP_SECS,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const WARLOCK_AT: Vec3 = Vec3::new(3.0, 1.0, -2.0);
const ENEMY_AT: Vec3 = Vec3::new(-6.0, 1.0, 4.0);
const CAMERA_AT: Vec3 = Vec3::new(10.0, 12.0, 14.0);

/// The client's two glow quads, `(width, height)` yd, and its pulse range.
const BIG_QUAD: Vec2 = Vec2::new(1.84, 2.38);
const SMALL_QUAD: Vec2 = Vec2::new(1.66, 2.14);
const PULSE_MIN: f32 = 0.67;
const PULSE_MAX: f32 = 0.99;

struct Harness {
    app: App,
    warlock: Entity,
    enemy: Entity,
    /// Frames run since the last [`Harness::tap`].
    frames_since_tap: u32,
}

impl Harness {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::transform::TransformPlugin,
        ));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
        app.insert_resource(AbilityDefinitions::default());
        app.add_systems(
            Update,
            (
                consume_instant_ability_signals,
                spawn_life_tap_glow_visuals,
                update_life_tap_glows,
                animate_client_particles,
                billboard_hunter_shots,
            )
                .chain(),
        );
        app.world_mut().spawn((
            Camera3d::default(),
            Transform::from_translation(CAMERA_AT).looking_at(WARLOCK_AT, Vec3::Y),
        ));
        let warlock = app
            .world_mut()
            .spawn((
                Combatant::new(1, 0, CharacterClass::Warlock),
                Transform::from_translation(WARLOCK_AT),
            ))
            .id();
        let enemy = app
            .world_mut()
            .spawn((
                Combatant::new(2, 0, CharacterClass::Warrior),
                Transform::from_translation(ENEMY_AT),
            ))
            .id();
        // Let the clock start, so the first tap frame has a real step.
        app.update();
        Harness {
            app,
            warlock,
            enemy,
            frames_since_tap: 0,
        }
    }

    fn fire(&mut self, caster: Entity, ability: AbilityType) {
        self.app.world_mut().spawn(InstantAbilityFired {
            caster,
            target: None,
            ability,
            is_crit: false,
        });
    }

    fn tap(&mut self) {
        self.fire(self.warlock, AbilityType::LifeTap);
        self.frames_since_tap = 0;
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
        self.frames_since_tap += frames;
    }

    /// Run until `secs` on the tap's own clock (one tick per frame from the
    /// frame the router sees the marker).
    fn run_to(&mut self, secs: f32) {
        let frames = (secs / TICK.as_secs_f32()).round() as u32;
        self.tick(frames.saturating_sub(self.frames_since_tap));
    }

    fn chest(&self, unit: Entity) -> Vec3 {
        self.app.world().get::<Transform>(unit).unwrap().translation + Vec3::Y * IMPACT_CHEST_Y
    }

    /// Every glow quad: world centre, world `(width, height)`, world facing
    /// (the quad's +Z) and its material's alpha.
    fn quads(&mut self) -> Vec<(Vec3, Vec2, Vec3, f32)> {
        let mut q = self.app.world_mut().query_filtered::<(
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
        ), With<LifeTapGlowQuad>>();
        let rows: Vec<(GlobalTransform, Handle<StandardMaterial>)> = q
            .iter(self.app.world())
            .map(|(g, m)| (*g, m.0.clone()))
            .collect();
        let materials = self.app.world().resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(g, m)| {
                let (scale, rotation, at) = g.to_scale_rotation_translation();
                let alpha = materials.get(&m).unwrap().base_color.alpha();
                (at, scale.truncate(), rotation * Vec3::Z, alpha)
            })
            .collect()
    }

    /// Every live client particle: its rendered colour, blend, world
    /// rotation and world size (the unit quad's scale), with how far through
    /// its life it is — the clock the bench's keys are read on.
    fn rendered(&mut self) -> Vec<Rendered> {
        let mut q = self.app.world_mut().query::<(
            Entity,
            &ClientParticle,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
        )>();
        let rows: Vec<(Entity, f32, GlobalTransform, Handle<StandardMaterial>)> = q
            .iter(self.app.world())
            .map(|(e, p, g, m)| (e, p.age / p.life, *g, m.0.clone()))
            .collect();
        let materials = self.app.world().resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(entity, t, g, m)| {
                let material = materials.get(&m).unwrap();
                let (scale, rotation, _) = g.to_scale_rotation_translation();
                Rendered {
                    entity,
                    t,
                    hue: hue(material.base_color.to_linear()),
                    additive: material.alpha_mode == AlphaMode::Add,
                    size: scale.x,
                    rotation,
                }
            })
            .collect()
    }

    fn camera_rotation(&mut self) -> Quat {
        let mut q = self
            .app
            .world_mut()
            .query_filtered::<&GlobalTransform, With<Camera3d>>();
        q.single(self.app.world()).unwrap().rotation()
    }

    /// Every live client particle: entity, world position, rendered colour.
    fn particles(&mut self) -> Vec<(Entity, Vec3, LinearRgba)> {
        let mut q = self.app.world_mut().query_filtered::<(
            Entity,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
        ), With<ClientParticle>>();
        let rows: Vec<(Entity, Vec3, Handle<StandardMaterial>)> = q
            .iter(self.app.world())
            .map(|(e, g, m)| (e, g.translation(), m.0.clone()))
            .collect();
        let materials = self.app.world().resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(e, at, m)| (e, at, materials.get(&m).unwrap().base_color.to_linear()))
            .collect()
    }
}

/// The colour family a particle renders in, by its dominant channel.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Hue {
    Red,
    Green,
    Cyan,
    Blue,
    Other,
}

fn hue(c: LinearRgba) -> Hue {
    let (r, g, b) = (c.red, c.green, c.blue);
    if r > 0.5 && g < 0.1 && b < 0.1 {
        Hue::Red
    } else if g > 2.0 * r && g > 2.0 * b {
        Hue::Green
    } else if b > 0.6 && g > 0.3 && r < 0.05 {
        Hue::Cyan
    } else if b > r && b > g {
        Hue::Blue
    } else {
        Hue::Other
    }
}

/// One rendered client particle.
#[derive(Debug, Clone, Copy)]
struct Rendered {
    entity: Entity,
    /// Age over life, 0..1.
    t: f32,
    hue: Hue,
    additive: bool,
    /// World size, yards: the unit quad's scale.
    size: f32,
    rotation: Quat,
}

/// The bench's `scale` keys at life 0, 0.5 and 1, per layer — sprite RADII.
const MOTE_SCALE: [f32; 3] = [0.03, 0.14, 0.28];
const TWINKLE_SCALE: [f32; 3] = [0.014, 0.11, 0.014];
const PUFF_SCALE: [f32; 3] = [0.11, 0.44, 1.11];
const SPARK_SCALE: [f32; 3] = [0.08, 0.06, 0.03];

/// The bench's sprite DIAMETER at `t` through a particle's life: twice its
/// radius key (`sc = keys(scale) * 2`, drawn at `r = sc * 0.5`).
fn bench_diameter(keys: [f32; 3], t: f32) -> f32 {
    let radius = if t < 0.5 {
        keys[0] + (keys[1] - keys[0]) * t / 0.5
    } else {
        keys[1] + (keys[2] - keys[1]) * (t - 0.5) / 0.5
    };
    radius * 2.0
}

fn harness() -> Harness {
    Harness::new()
}

impl Harness {
    fn tapped() -> Self {
        let mut h = harness();
        h.tap();
        h
    }
}

#[test]
fn the_glow_plays_on_the_warlocks_own_chest() {
    let mut h = Harness::tapped();
    h.run_to(1.334);
    let quads = h.quads();
    assert_eq!(quads.len(), 2, "two glow quads: {quads:?}");
    let chest = h.chest(h.warlock);
    let enemy = h.chest(h.enemy);
    for (at, ..) in &quads {
        assert!(
            at.distance(chest) < 1e-3,
            "a quad sits at {at}, not the Warlock's chest {chest}"
        );
        assert!(at.distance(enemy) > 5.0);
    }
}

/// The control: another caster-centred instant (Frost Nova) and a targeted
/// one (Power Word: Shield) land no Life Tap glow.
#[test]
fn only_life_tap_lands_the_glow() {
    let mut h = harness();
    h.fire(h.warlock, AbilityType::FrostNova);
    h.app.world_mut().spawn(InstantAbilityFired {
        caster: h.warlock,
        target: Some(h.enemy),
        ability: AbilityType::PowerWordShield,
        is_crit: false,
    });
    h.tick(90);
    assert!(h.quads().is_empty());
    h.tap();
    h.tick(2);
    assert_eq!(h.quads().len(), 2);
}

#[test]
fn the_glow_is_body_tall_and_throbs_on_its_833_ms_loop() {
    let mut h = Harness::tapped();
    h.run_to(0.9);
    let (mut lo, mut hi) = (f32::MAX, 0.0f32);
    // One full loop and a bit, frame by frame.
    for _ in 0..60 {
        h.tick(1);
        let quads = h.quads();
        let big = quads
            .iter()
            .map(|q| q.1)
            .max_by(|a, b| a.x.total_cmp(&b.x))
            .unwrap();
        let small = quads
            .iter()
            .map(|q| q.1)
            .min_by(|a, b| a.x.total_cmp(&b.x))
            .unwrap();
        // Each quad keeps the client's proportions at every pulse.
        assert!((big.x / big.y - BIG_QUAD.x / BIG_QUAD.y).abs() < 1e-3);
        assert!((small.x / small.y - SMALL_QUAD.x / SMALL_QUAD.y).abs() < 1e-3);
        assert!((small.x / big.x - SMALL_QUAD.x / BIG_QUAD.x).abs() < 1e-3);
        lo = lo.min(big.y);
        hi = hi.max(big.y);
    }
    // About a body tall at its widest, and the throb spans the client keys:
    // never outside them, and within one 16 ms frame of each extreme.
    let (min, max) = (BIG_QUAD.y * PULSE_MIN, BIG_QUAD.y * PULSE_MAX);
    assert!(
        lo >= min - 1e-3 && lo - min < 0.08,
        "smallest {lo} vs {min}"
    );
    assert!(hi <= max + 1e-3 && max - hi < 0.08, "largest {hi} vs {max}");
}

#[test]
fn the_glow_swells_to_full_at_1334_ms_and_is_gone_by_3334_ms() {
    let mut h = Harness::tapped();
    let alpha = |h: &mut Harness| h.quads()[0].3;
    h.run_to(0.1);
    let early = alpha(&mut h);
    h.run_to(0.67);
    let half = alpha(&mut h);
    h.run_to(1.334);
    let peak = alpha(&mut h);
    h.run_to(2.334);
    let fading = alpha(&mut h);
    assert!(early < 0.1, "{early}");
    assert!((half - 0.5).abs() < 0.05, "{half}");
    assert!(peak > 0.97, "{peak}");
    assert!((fading - 0.5).abs() < 0.05, "{fading}");
    h.run_to(LIFE_TAP_SECS + 0.05);
    assert!(h.quads().is_empty(), "the glow outlived its 3.334 s");
}

#[test]
fn the_glow_faces_the_camera() {
    let mut h = Harness::tapped();
    h.run_to(1.0);
    let camera_back = (CAMERA_AT - WARLOCK_AT).normalize();
    let quads = h.quads();
    assert_eq!(quads.len(), 2, "two glow quads: {quads:?}");
    for (_, _, normal, _) in quads {
        assert!(
            normal.dot(camera_back) > 0.99,
            "a quad faces {normal}, the camera is along {camera_back}"
        );
    }
}

#[test]
fn the_glow_rides_the_warlock() {
    let mut h = Harness::tapped();
    h.run_to(0.5);
    h.app
        .world_mut()
        .get_mut::<Transform>(h.warlock)
        .unwrap()
        .translation += Vec3::new(4.0, 0.0, 3.0);
    h.tick(2);
    let chest = h.chest(h.warlock);
    let quads = h.quads();
    assert_eq!(quads.len(), 2, "two glow quads: {quads:?}");
    for (at, ..) in quads {
        assert!(at.distance(chest) < 1e-3, "{at} left the chest {chest}");
    }
}

/// The red-to-blue pull: motes are born red on the bench's 0.7 yd shell
/// (0.42-0.70 yd out), close on the chest, and render blue late in their
/// lives.
#[test]
fn red_motes_close_on_the_chest_and_turn_blue() {
    let mut h = Harness::tapped();
    h.run_to(0.8);
    assert!(
        !h.particles().iter().any(|p| hue(p.2) == Hue::Red),
        "no mote before 834 ms"
    );
    h.run_to(1.0);
    let chest = h.chest(h.warlock);
    let red: Vec<(Entity, Vec3)> = h
        .particles()
        .into_iter()
        .filter(|p| hue(p.2) == Hue::Red)
        .map(|p| (p.0, p.1))
        .collect();
    assert!(red.len() >= 3, "red motes by 1.0 s: {}", red.len());
    // Born on the shell: the newest are still near their birth radius.
    let far = red
        .iter()
        .map(|(_, at)| at.distance(chest))
        .fold(0.0f32, f32::max);
    assert!((0.35..=0.71).contains(&far), "outermost mote {far} yd out");
    // Closing: every red mote nearer the chest a few frames on.
    let before: Vec<f32> = red.iter().map(|(_, at)| at.distance(chest)).collect();
    h.tick(3);
    let now = h.particles();
    let mut checked = 0;
    for ((e, _), d0) in red.iter().zip(before) {
        if d0 < 0.25 {
            continue; // passing through the centre
        }
        let at = now.iter().find(|p| p.0 == *e).unwrap().1;
        assert!(at.distance(chest) < d0, "a mote moved away from the chest");
        checked += 1;
    }
    assert!(checked > 0, "no mote far enough out to measure");
    // Late in their lives the motes render blue.
    h.run_to(2.0);
    assert!(
        h.particles().iter().any(|p| hue(p.2) == Hue::Blue),
        "no blue mote by 2.0 s"
    );
}

/// The green puff and sparks burst in the first third of a second; the puff
/// stalls under its drag instead of flying out.
#[test]
fn the_green_puff_bursts_and_stalls() {
    let mut h = Harness::tapped();
    h.run_to(0.1);
    let chest = h.chest(h.warlock);
    let green: Vec<Entity> = h
        .particles()
        .into_iter()
        .filter(|p| hue(p.2) == Hue::Green)
        .map(|p| p.0)
        .collect();
    assert!(green.len() >= 10, "green puff and sparks: {}", green.len());
    h.run_to(0.6);
    let now = h.particles();
    let reach: Vec<f32> = green
        .iter()
        .filter_map(|e| now.iter().find(|p| p.0 == *e))
        .map(|p| p.1.distance(chest))
        .collect();
    // The sparks (1.39 yd/s, no drag) are ~0.6 yd out by now; the puff,
    // launched faster, has stalled within a few tenths of a yard.
    let stalled = reach.iter().filter(|d| **d < 0.2).count();
    let flying = reach.iter().filter(|d| **d > 0.5).count();
    assert!(stalled >= 5, "stalled puffs: {reach:?}");
    assert!(flying >= 5, "flying sparks: {reach:?}");
    // Nothing green is born after 334 ms.
    let before: Vec<Entity> = h.particles().iter().map(|p| p.0).collect();
    h.run_to(0.9);
    assert!(!h
        .particles()
        .iter()
        .any(|p| !before.contains(&p.0) && hue(p.2) == Hue::Green));
}

#[test]
fn the_cyan_twinkles_wait_for_their_gate() {
    let mut h = Harness::tapped();
    h.run_to(0.98);
    assert!(!h.particles().iter().any(|p| hue(p.2) == Hue::Cyan));
    h.run_to(1.1);
    assert!(h.particles().iter().any(|p| hue(p.2) == Hue::Cyan));
    // Between the second and third gates (1.5-1.634 s) the 0.2 s twinkles
    // born before 1.5 s run out, and none are born.
    h.run_to(1.62);
    let gap = h
        .particles()
        .iter()
        .filter(|p| hue(p.2) == Hue::Cyan)
        .count();
    h.run_to(1.75);
    let open = h
        .particles()
        .iter()
        .filter(|p| hue(p.2) == Hue::Cyan)
        .count();
    assert!(
        open > gap,
        "gate 3 opens on more twinkles ({open}) than the gap ({gap})"
    );
}

// ── The real Warlock decision ──────────────────────────────────────────────

const WARLOCK: &str = "warlock";
const ENEMY: &str = "enemy warrior";

/// A starved Warlock at full health, an enemy in Corruption's range: it taps
/// (`tests/life_tap_scenes.rs`). `mana` is its pool fraction.
fn tap_scene(mana: f32) -> Scene {
    Scene::new()
        .unit(WARLOCK, 1, CharacterClass::Warlock)
        .mana(mana)
        .targeting(ENEMY)
        .unit(ENEMY, 2, CharacterClass::Warrior)
        .at(20.0, 0.0)
}

/// Replay a decision's markers through the router against stand-ins.
fn glow_from_decision(view: &DecisionView) -> (Harness, Vec<(Vec3, Vec2, Vec3, f32)>) {
    let mut h = harness();
    for marker in &view.instant_markers {
        let named = |who: &str| match who {
            WARLOCK => h.warlock,
            ENEMY => h.enemy,
            other => panic!("the decision named {other}, who has no stand-in"),
        };
        let (caster, target) = (named(marker.caster), marker.target.map(named));
        h.app.world_mut().spawn(InstantAbilityFired {
            caster,
            target,
            ability: marker.ability,
            is_crit: false,
        });
    }
    h.tick(2);
    let quads = h.quads();
    (h, quads)
}

/// The real Warlock AI's Life Tap lights the glow on the WARLOCK; the pair's
/// control, the same Warlock with mana to spare, holds its tap and lights
/// nothing.
#[test]
fn the_warlocks_own_tap_lights_its_chest() {
    let view = tap_scene(0.2).decide(WARLOCK);
    assert_eq!(view.chosen, Some((AbilityType::LifeTap, None)));
    let (h, quads) = glow_from_decision(&view);
    assert_eq!(quads.len(), 2, "the tap's glow: {quads:?}");
    let chest = h.chest(h.warlock);
    for (at, ..) in quads {
        assert!(
            at.distance(chest) < 1e-3,
            "the glow is at {at}, not {chest}"
        );
    }

    let control = tap_scene(1.0).decide(WARLOCK);
    assert_ne!(control.chosen.map(|c| c.0), Some(AbilityType::LifeTap));
    assert!(glow_from_decision(&control).1.is_empty());
}

/// Every layer's particles render at the bench's size: twice its `scale`
/// key, which is a radius, at that particle's point in its life.
#[test]
fn every_layer_renders_at_the_benchs_size() {
    let mut h = Harness::tapped();
    let check = |rows: &[Rendered], name: &str, keys: [f32; 3]| {
        assert!(!rows.is_empty(), "no {name} to measure");
        for p in rows {
            let want = bench_diameter(keys, p.t);
            assert!(
                (p.size - want).abs() < 1e-3 + want * 1e-3,
                "a {name} renders {} yd at t {}, the bench {want}",
                p.size,
                p.t
            );
        }
    };
    // The green burst: the puff alpha-blended, the sparks additive.
    h.run_to(0.25);
    let rows = h.rendered();
    let green: Vec<Rendered> = rows
        .iter()
        .copied()
        .filter(|p| p.hue == Hue::Green)
        .collect();
    let puff: Vec<Rendered> = green.iter().copied().filter(|p| !p.additive).collect();
    let sparks: Vec<Rendered> = green.iter().copied().filter(|p| p.additive).collect();
    check(&puff, "puff", PUFF_SCALE);
    check(&sparks, "spark", SPARK_SCALE);
    // The motes (red early in their lives) and the first gate's twinkles.
    h.run_to(1.1);
    let rows = h.rendered();
    let motes: Vec<Rendered> = rows.iter().copied().filter(|p| p.hue == Hue::Red).collect();
    let twinkles: Vec<Rendered> = rows
        .iter()
        .copied()
        .filter(|p| p.hue == Hue::Cyan)
        .collect();
    check(&motes, "mote", MOTE_SCALE);
    check(&twinkles, "twinkle", TWINKLE_SCALE);
}

/// The twinkles spin in the view plane at the bench's -0.64 turns a second.
#[test]
fn the_twinkles_spin_at_the_benchs_rate() {
    let mut h = Harness::tapped();
    h.run_to(1.04);
    let camera = h.camera_rotation();
    // A young twinkle, so it outlives the measurement.
    let young = h
        .rendered()
        .into_iter()
        .filter(|p| p.hue == Hue::Cyan && p.t < 0.4)
        .collect::<Vec<_>>();
    assert!(!young.is_empty(), "no young twinkle at 1.04 s");
    // The roll about the view axis, read off the world rotation.
    let roll = |rotation: Quat| {
        let rel = camera.inverse() * rotation;
        assert!(
            (rel * Vec3::Z).dot(Vec3::Z) > 0.999,
            "a twinkle turned off the camera's facing"
        );
        let x = rel * Vec3::X;
        x.y.atan2(x.x)
    };
    let before: Vec<(Entity, f32)> = young.iter().map(|p| (p.entity, roll(p.rotation))).collect();
    const FRAMES: u32 = 3;
    h.tick(FRAMES);
    let now = h.rendered();
    let secs = FRAMES as f32 * TICK.as_secs_f32();
    let want = -0.64 * std::f32::consts::TAU * secs;
    for (e, r0) in before {
        let p = now
            .iter()
            .find(|p| p.entity == e)
            .expect("the twinkle lived on");
        let mut turned = roll(p.rotation) - r0;
        // Wrap into (-pi, pi].
        turned = (turned + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        assert!(
            (turned - want).abs() < 0.01,
            "a twinkle turned {turned} rad in {secs} s, the bench {want}"
        );
    }
}

/// A landing whose Warlock is gone goes with it: the glow and both its quads.
#[test]
fn the_glow_goes_with_its_warlock() {
    let mut h = Harness::tapped();
    h.run_to(0.5);
    assert_eq!(h.quads().len(), 2);
    h.app.world_mut().entity_mut(h.warlock).despawn();
    h.tick(2);
    assert!(
        h.quads().is_empty(),
        "the glow's quads outlived its Warlock"
    );
    let mut glows = h.app.world_mut().query::<&LifeTapGlow>();
    assert_eq!(glows.iter(h.app.world()).count(), 0, "an orphaned landing");
}

/// The glow is the bench's radial gradient in the bench's own blue: alpha
/// 0.55 at the centre, 0.25 at half radius, 0 at the rim (under the alpha
/// track), lit by the colour itself with no gain over it.
#[test]
fn the_glow_draws_the_benchs_gradient_in_its_blue() {
    let mut h = Harness::tapped();
    h.run_to(0.5);
    let mut q = h
        .app
        .world_mut()
        .query_filtered::<&MeshMaterial3d<StandardMaterial>, With<LifeTapGlowQuad>>();
    let handles: Vec<Handle<StandardMaterial>> =
        q.iter(h.app.world()).map(|m| m.0.clone()).collect();
    assert_eq!(handles.len(), 2);
    let world = h.app.world();
    let materials = world.resource::<Assets<StandardMaterial>>();
    let images = world.resource::<Assets<Image>>();
    let blue = Color::srgb_u8(81, 101, 255).to_linear();
    for handle in handles {
        let material = materials.get(&handle).unwrap();
        assert_eq!(material.alpha_mode, AlphaMode::Add);
        let e = material.emissive;
        assert!(
            (e.red - blue.red).abs() < 1e-4
                && (e.green - blue.green).abs() < 1e-4
                && (e.blue - blue.blue).abs() < 1e-4,
            "the glow is lit {e:?}, its colour is {blue:?}"
        );
        let texture = material.base_color_texture.as_ref().expect("a gradient");
        let image = images.get(texture).unwrap();
        let (w, hgt) = (image.width() as usize, image.height() as usize);
        let data = image.data.as_ref().unwrap();
        // Alpha along the row through the centre, by radius.
        let row = hgt / 2;
        let alpha_at = |r: f32| {
            let x = ((r + 1.0) * 0.5 * w as f32).floor().min(w as f32 - 1.0) as usize;
            data[(row * w + x) * 4 + 3] as f32 / 255.0
        };
        for (r, want) in [(0.0, 0.55), (0.5, 0.25), (0.999, 0.0)] {
            let got = alpha_at(r);
            assert!(
                (got - want).abs() < 0.03,
                "the glow's alpha at radius {r} is {got}, the bench {want}"
            );
        }
    }
}
