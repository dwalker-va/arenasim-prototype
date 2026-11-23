//! Probes for the Priest's Shadow cast hands (`rendering/effects/heal_cast.rs`,
//! `HealCastKind::Shadow`, AS-449): `shadow_precast_low_hand.m2` on both
//! spell hands for Shadow Word: Pain, Vampiric Embrace and Silence (played
//! off their `InstantAbilityFired`) and through Mind Blast's cast bar.
//!
//! What is asserted is what is DRAWN, in world space through
//! `GlobalTransform` and the spawned materials, meshes and textures, against
//! an independent transcription of the parsed model below; the transcription
//! reads none of the module's constants. Every particle's age is checked
//! against the frame clock (frames x dt) before anything is read off it.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin`: no window,
//! no GPU. The hand systems run in the order `states/mod.rs` registers them
//! (the FixedUpdate readers first, then the Update chain); the production
//! schedule itself is played by `shadow_cast_hands_match_flow.rs`.

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::render::mesh::MeshAabb;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::scene::{DecisionView, Scene};
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraPending, AuraType, CastEnding, CastEndingKind, CastingOrb, CastingState,
    Combatant, HealCastHand, HealCastKind, HealCastPhase, HealCastShadowMote, InstantAbilityFired,
    PlayMatchEntity, PolymorphedVisual, VisualBody,
};
use arenasim::states::play_match::{
    billboard_heal_cast_glows, cleanup_heal_cast_glows, cleanup_play_match,
    consume_cast_ending_signals, consume_heal_cast_endings, spawn_casting_orbs,
    spawn_heal_cast_glows, spawn_instant_cast_hands, update_heal_cast_glows,
    update_heal_cast_posture,
};
use arenasim::CharacterClass;

// ── the model, transcribed ─────────────────────────────────────────────────
//
// `spells/shadow_precast_low_hand.m2` (fdid 166807), parsed at build
// 1.15.9.69547 (AS-449): three point emitters at the hand, every ramp keyed at
// 0, 0.5 and 1 of the life, each size key a sprite RADIUS.

#[derive(Clone, Copy, Debug)]
struct Emitter {
    rate: f32,
    life: f32,
    speed: f32,
    radii: [f32; 3],
    rgb: [[f32; 3]; 3],
    alpha: [f32; 3],
    additive: bool,
}

const LICK: Emitter = Emitter {
    rate: 10.2,
    life: 1.2,
    speed: 0.333,
    radii: [0.222, 0.083, 0.028],
    rgb: [[32.0, 255.0, 2.0], [2.0, 2.0, 2.0], [2.0, 2.0, 2.0]],
    alpha: [0.392, 1.0, 0.0],
    additive: false,
};
const GREEN_RING: Emitter = Emitter {
    rate: 5.0,
    life: 0.5,
    speed: 0.0,
    radii: [0.222, 0.361, 0.222],
    rgb: [[18.0, 255.0, 0.0], [10.0, 148.0, 0.0], [18.0, 255.0, 0.0]],
    alpha: [1.0, 1.0, 1.0],
    additive: true,
};
const VIOLET_RING: Emitter = Emitter {
    rate: 5.0,
    life: 0.5,
    speed: 0.0,
    radii: [0.278, 0.417, 0.278],
    rgb: [[114.0, 0.0, 255.0], [55.0, 0.0, 124.0], [114.0, 0.0, 255.0]],
    alpha: [1.0, 1.0, 1.0],
    additive: true,
};
const EMITTERS: [Emitter; 3] = [LICK, GREEN_RING, VIOLET_RING];
/// The cast kit plays the model's one 1000 ms sequence; then the rig waits
/// out the longest life (the lick's 1.2 s).
const KIT_SECS: f32 = 1.0;
const RIG_SECS: f32 = 2.2;
/// The spell-hand socket on the `VisualBody` (`heal_cast.rs`'s, shared by
/// every hand family).
const SOCKET: Vec3 = Vec3::new(0.72, 0.05, 0.30);

fn key3(k: [f32; 3], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        k[0] + (k[1] - k[0]) * t / 0.5
    } else {
        k[1] + (k[2] - k[1]) * (t - 0.5) / 0.5
    }
}

/// The diameter a particle of `e` is drawn at `age` seconds in.
fn diameter(e: &Emitter, age: f32) -> f32 {
    2.0 * key3(e.radii, age / e.life)
}

/// The ages, in birth order, of the particles of `e` alive `t` seconds after
/// the cast kit began (emitting for `emit` seconds of it): one per filled
/// accumulator, the first `1 / rate` in.
fn live_ages(e: &Emitter, t: f32, emit: f32) -> Vec<f32> {
    let born = (e.rate * emit.min(t) + 1e-4).floor() as u32;
    (1..=born)
        .map(|k| t - k as f32 / e.rate)
        .filter(|age| *age < e.life)
        .collect()
}

// ── the harness ────────────────────────────────────────────────────────────

struct Harness {
    app: App,
    dt: f32,
    frames: u32,
}

impl Harness {
    fn new(tick: Duration) -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::transform::TransformPlugin,
        ));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(tick));
        app.insert_resource(AbilityDefinitions::default());
        app.add_systems(
            Update,
            (
                consume_heal_cast_endings,
                spawn_instant_cast_hands,
                consume_cast_ending_signals,
                spawn_casting_orbs,
                spawn_heal_cast_glows,
                update_heal_cast_glows,
                cleanup_heal_cast_glows,
                update_heal_cast_posture,
                billboard_heal_cast_glows,
            )
                .chain(),
        );
        // The first update only starts the clock (`Time` reads zero delta).
        app.update();
        Harness {
            app,
            dt: tick.as_secs_f32(),
            frames: 0,
        }
    }

    fn at_16ms() -> Self {
        Self::new(Duration::from_millis(16))
    }

    fn camera(&mut self, from: Vec3, look_at: Vec3) -> Quat {
        let transform = Transform::from_translation(from).looking_at(look_at, Vec3::Y);
        self.app.world_mut().spawn((Camera3d::default(), transform));
        transform.rotation
    }

    /// A Priest as `spawn_combatant` builds it: a `PlayMatchEntity` at the
    /// game's capsule height with a `VisualBody` child.
    fn priest(&mut self, at: Vec3) -> (Entity, Entity) {
        let world = self.app.world_mut();
        let priest = world
            .spawn((
                Combatant::new(1, 0, CharacterClass::Priest),
                Transform::from_translation(at),
                ActiveAuras::default(),
                PlayMatchEntity,
            ))
            .id();
        let body = world
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        world.entity_mut(priest).add_child(body);
        (priest, body)
    }

    fn fire(&mut self, priest: Entity, ability: AbilityType) {
        self.app.world_mut().spawn((
            InstantAbilityFired {
                caster: priest,
                target: Some(priest),
                ability,
                is_crit: false,
            },
            PlayMatchEntity,
        ));
    }

    fn begin_mind_blast(&mut self, priest: Entity) {
        let cast_time = AbilityDefinitions::default()
            .get_unchecked(&AbilityType::MindBlast)
            .cast_time;
        self.app
            .world_mut()
            .entity_mut(priest)
            .insert(CastingState::new(AbilityType::MindBlast, priest, cast_time));
    }

    /// The cast ends as `process_casting` and the interrupt sites end it.
    fn end_cast(&mut self, priest: Entity, kind: CastEndingKind) {
        if kind == CastEndingKind::Interrupted {
            let mut state = self
                .app
                .world_mut()
                .get_mut::<CastingState>(priest)
                .unwrap();
            state.interrupted = true;
            state.interrupted_display_time = 0.5;
        } else {
            self.app
                .world_mut()
                .entity_mut(priest)
                .remove::<CastingState>();
        }
        self.app.world_mut().spawn(CastEnding {
            caster: priest,
            kind,
        });
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
            self.frames += 1;
        }
    }

    fn rigs(&mut self) -> Vec<RigView> {
        let mut q = self
            .app
            .world_mut()
            .query::<(Entity, &HealCastHand, &ChildOf, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(e, h, c, g)| RigView {
                entity: e,
                kind: h.kind,
                caster: h.caster,
                parent: c.parent(),
                side: h.side,
                age: h.age,
                phase: h.phase,
                at: g.translation(),
            })
            .collect()
    }

    fn motes(&mut self) -> Vec<MoteView> {
        let mut q = self.app.world_mut().query::<(
            &HealCastShadowMote,
            &ChildOf,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
            &Mesh3d,
        )>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .map(|(m, c, g, mat, mesh)| {
                (
                    m.emitter,
                    m.age,
                    c.parent(),
                    *g,
                    mat.0.clone(),
                    mesh.0.clone(),
                )
            })
            .collect();
        let materials = self.app.world().resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(emitter, age, rig, g, mat, mesh)| MoteView {
                emitter,
                age,
                rig,
                global: g,
                material: materials.get(&mat).unwrap().clone(),
                mesh,
            })
            .collect()
    }

    fn count<C: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query_filtered::<(), With<C>>();
        q.iter(self.app.world()).count()
    }
}

#[derive(Clone, Copy, Debug)]
struct RigView {
    entity: Entity,
    kind: HealCastKind,
    caster: Entity,
    parent: Entity,
    side: f32,
    age: f32,
    phase: HealCastPhase,
    at: Vec3,
}

#[derive(Clone, Debug)]
struct MoteView {
    emitter: usize,
    age: f32,
    rig: Entity,
    global: GlobalTransform,
    material: StandardMaterial,
    mesh: Handle<Mesh>,
}

/// A frame count whose clock lands exactly on `secs` at every tick probed
/// (8, 16 and 33 ms all divide 0.528 s).
fn frames_for(secs: f32, dt: f32) -> u32 {
    let n = (secs / dt).round();
    assert!(
        (n * dt - secs).abs() < 1e-6,
        "{secs} s is not a whole number of {dt} s frames"
    );
    n as u32
}

fn ages_of(motes: &[MoteView], rig: Entity, emitter: usize) -> Vec<f32> {
    let mut ages: Vec<f32> = motes
        .iter()
        .filter(|m| m.rig == rig && m.emitter == emitter)
        .map(|m| m.age)
        .collect();
    ages.sort_by(|a, b| b.partial_cmp(a).unwrap());
    ages
}

fn assert_ages(label: &str, got: &[f32], want: &[f32]) {
    assert_eq!(
        got.len(),
        want.len(),
        "{label}: {} particles drawn, {} owed ({got:?} vs {want:?})",
        got.len(),
        want.len()
    );
    for (g, w) in got.iter().zip(want) {
        assert!(
            (g - w).abs() < 1e-4,
            "{label}: age {g} where the clock says {w}"
        );
    }
}

// ── routing ────────────────────────────────────────────────────────────────

/// Exactly the Priest's four Shadow casts reach the Shadow hands (set
/// equality over the whole config), the three instants through their
/// marker and Mind Blast through its cast bar's loop.
#[test]
fn exactly_the_priests_shadow_casts_reach_the_shadow_hands() {
    let defs = AbilityDefinitions::default();
    let mut routed: Vec<AbilityType> = defs
        .iter()
        .map(|(a, _)| *a)
        .filter(|a| HealCastKind::for_ability(*a) == Some(HealCastKind::Shadow))
        .collect();
    routed.sort();
    let mut want = vec![
        AbilityType::ShadowWordPain,
        AbilityType::VampiricEmbrace,
        AbilityType::Silence,
        AbilityType::MindBlast,
    ];
    want.sort();
    assert_eq!(routed, want);
    for a in [
        AbilityType::ShadowWordPain,
        AbilityType::VampiricEmbrace,
        AbilityType::Silence,
    ] {
        assert!(
            InstantAbilityFired::is_spawned_for(a),
            "{a:?} needs its marker"
        );
        assert_eq!(defs.get_unchecked(&a).cast_time, 0.0, "{a:?} is an instant");
    }
    assert!(!InstantAbilityFired::is_spawned_for(AbilityType::MindBlast));
    assert!(defs.get_unchecked(&AbilityType::MindBlast).cast_time > 0.0);
    assert!(
        HealCastKind::Shadow.has_precast(),
        "Mind Blast's kit 114 loops"
    );
}

// ── what is drawn ──────────────────────────────────────────────────────────

/// One Shadow Word: Pain lights both spell hands with the model's three
/// emitters and nothing else: at 0.528 s each hand carries exactly the
/// licks and rings the clock owes, each at its age's size on a unit quad,
/// a lick risen straight up from the socket at its speed (±35%), a ring
/// still on it.
#[test]
fn an_instant_draws_the_models_three_emitters_on_both_hands() {
    let mut h = Harness::at_16ms();
    let (priest, body) = h.priest(Vec3::new(2.0, 1.0, -3.0));
    h.fire(priest, AbilityType::ShadowWordPain);
    let frames = frames_for(0.528, h.dt);
    h.tick(frames);
    let t = frames as f32 * h.dt;

    let rigs = h.rigs();
    assert_eq!(rigs.len(), 2, "both hands: {rigs:?}");
    let mut sides: Vec<f32> = rigs.iter().map(|r| r.side).collect();
    sides.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(sides, vec![-1.0, 1.0]);
    for r in &rigs {
        assert_eq!(r.kind, HealCastKind::Shadow);
        assert_eq!(r.caster, priest);
        assert_eq!(r.parent, body, "the rig hangs off the rendered body");
        assert!(
            matches!(r.phase, HealCastPhase::Flare { .. }),
            "an instant plays its cast kit"
        );
        assert!(
            (r.age - t).abs() < 1e-5,
            "rig age {} after {frames} frames of {}",
            r.age,
            h.dt
        );
    }

    let motes = h.motes();
    let meshes = h.app.world().resource::<Assets<Mesh>>();
    let mut speeds = Vec::new();
    for r in &rigs {
        for (i, e) in EMITTERS.iter().enumerate() {
            let want = live_ages(e, t, KIT_SECS);
            assert!(
                !want.is_empty(),
                "the probe owes emitter {i} a particle by {t} s"
            );
            assert_ages(
                &format!("hand {} emitter {i}", r.side),
                &ages_of(&motes, r.entity, i),
                &want,
            );
        }
        for m in motes.iter().filter(|m| m.rig == r.entity) {
            let e = &EMITTERS[m.emitter];
            let (scale, _, at) = m.global.to_scale_rotation_translation();
            let d = diameter(e, m.age);
            assert!(
                (scale - Vec3::splat(d)).abs().max_element() < 1e-4,
                "emitter {} at {:.3} s drawn {scale}, the model's diameter is {d}",
                m.emitter,
                m.age
            );
            let aabb = meshes.get(&m.mesh).unwrap().compute_aabb().unwrap();
            assert!(
                (Vec3::from(aabb.half_extents).truncate() - Vec2::splat(0.5))
                    .abs()
                    .max_element()
                    < 1e-6,
                "a particle is a unit quad"
            );
            let socket = r.at;
            let rise = at - socket;
            if e.speed == 0.0 {
                assert!(rise.length() < 1e-4, "a ring stays on the hand: {rise}");
            } else {
                assert!(
                    rise.x.abs() < 1e-4 && rise.z.abs() < 1e-4,
                    "a lick rises straight up: {rise}"
                );
                let v = rise.y / m.age.max(1e-6);
                assert!(
                    v >= e.speed * 0.65 - 1e-3 && v <= e.speed * 1.35 + 1e-3,
                    "a lick rises at {v} yd/s, the model says {} ±35%",
                    e.speed
                );
                speeds.push(v / e.speed);
            }
        }
    }
    // The ±35% is drawn: the licks do not all rise at one speed.
    let (slow, fast) = speeds
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    assert!(speeds.len() >= 10, "{} licks", speeds.len());
    assert!(
        slow < 0.85 && fast > 1.15,
        "lick speeds span {slow:.2}..{fast:.2} of the model's, not ±35%"
    );
    // Nothing else hangs off a Shadow hand.
    let pieces = h.count::<arenasim::states::play_match::components::HealCastPiece>();
    assert_eq!(pieces, 0, "the model has no mesh: no glow or wisp pieces");
}

/// The licks blend (blend 2: they can darken to the black smoke), the rings
/// add (blend 4, glowing); each particle wears its ramp's colour and alpha
/// at its palette step, on the sprite decoded from its texture.
#[test]
fn the_licks_blend_the_rings_add_and_each_wears_its_ramp() {
    let mut h = Harness::at_16ms();
    let (priest, _) = h.priest(Vec3::new(0.0, 1.0, 0.0));
    h.fire(priest, AbilityType::VampiricEmbrace);
    let mut seen = [0usize; 3];
    for _ in 0..60 {
        h.tick(1);
        for m in h.motes() {
            let e = &EMITTERS[m.emitter];
            seen[m.emitter] += 1;
            let material = &m.material;
            if e.additive {
                assert_eq!(material.alpha_mode, AlphaMode::Add, "ring {}", m.emitter);
                assert!(
                    !material.unlit,
                    "unlit drops an additive particle's emissive"
                );
                assert!(
                    material.emissive.red + material.emissive.green + material.emissive.blue > 0.0
                );
            } else {
                assert_eq!(material.alpha_mode, AlphaMode::Blend, "the lick blends");
                assert!(material.unlit, "a blended particle is a flat colour");
            }
            // The palette's step is the one the particle's age is in, sampled
            // at the step's middle.
            let step = ((m.age / e.life) * 8.0).floor().min(7.0);
            let t = (step + 0.5) / 8.0;
            let c = material.base_color.to_srgba();
            let want = [0, 1, 2].map(|i| key3([e.rgb[0][i], e.rgb[1][i], e.rgb[2][i]], t) / 255.0);
            for (got, want) in [c.red, c.green, c.blue].iter().zip(want) {
                assert!(
                    (got - want).abs() < 1.5 / 255.0,
                    "emitter {} colour {c:?} at step {step}",
                    m.emitter
                );
            }
            assert!(
                (c.alpha - key3(e.alpha, t)).abs() < 1e-4,
                "emitter {} alpha {}",
                m.emitter,
                c.alpha
            );
        }
    }
    assert!(seen.iter().all(|n| *n > 20), "every emitter drew: {seen:?}");
}

/// The sprites are the decoded textures' radial profiles: the lick a soft
/// puff full at its centre, the ring dark inside 0.35 of the radius and
/// brightest at 0.72, white in colour with the shape in alpha.
#[test]
fn the_sprites_are_the_decoded_textures_profiles() {
    let mut h = Harness::at_16ms();
    let (priest, _) = h.priest(Vec3::new(0.0, 1.0, 0.0));
    h.fire(priest, AbilityType::Silence);
    h.tick(40);
    let motes = h.motes();
    let images = h.app.world().resource::<Assets<Image>>();
    let alpha_at = |m: &MoteView, r: f32| {
        let image = images
            .get(m.material.base_color_texture.as_ref().unwrap())
            .unwrap();
        let px = image.width() as usize;
        let data = image.data.as_ref().unwrap();
        let y = px / 2;
        let x = (((1.0 + r) * 0.5 * px as f32) as usize).min(px - 1);
        let i = (y * px + x) * 4;
        assert_eq!(&data[i..i + 3], &[255, 255, 255], "the sprite is white");
        data[i + 3] as f32 / 255.0
    };
    let lick = motes.iter().find(|m| m.emitter == 0).expect("a lick");
    let ring = motes.iter().find(|m| m.emitter == 1).expect("a ring");
    // `fire1a2`'s alpha averaged by radius: 0.96 at 0.15, 0.62 at 0.475,
    // gone by the edge.
    assert!(
        (alpha_at(lick, 0.15) - 0.96).abs() < 0.03,
        "{}",
        alpha_at(lick, 0.15)
    );
    assert!(
        (alpha_at(lick, 0.475) - 0.62).abs() < 0.03,
        "{}",
        alpha_at(lick, 0.475)
    );
    assert!(alpha_at(lick, 0.97) < 0.02);
    // `shockwave10`'s luminance: nothing inside, 0.24 at 0.725.
    assert!(alpha_at(ring, 0.0) < 0.01 && alpha_at(ring, 0.3) < 0.01);
    assert!(
        (alpha_at(ring, 0.725) - 0.24).abs() < 0.02,
        "{}",
        alpha_at(ring, 0.725)
    );
    assert!(alpha_at(ring, 0.6) < alpha_at(ring, 0.725));
    assert!(alpha_at(ring, 0.9) < alpha_at(ring, 0.725));
    // The two rings share the sprite; the lick does not.
    let violet = motes
        .iter()
        .find(|m| m.emitter == 2)
        .expect("a violet ring");
    assert_eq!(
        ring.material.base_color_texture,
        violet.material.base_color_texture
    );
    assert_ne!(
        ring.material.base_color_texture,
        lick.material.base_color_texture
    );
}

// ── facing ─────────────────────────────────────────────────────────────────

/// Every particle faces the camera square: its quad's normal is the
/// camera's back vector and its X axis the screen's, with no roll at all —
/// the model gives its emitters no spin — whatever the caster's facing and
/// the cast posture's lean.
#[test]
fn every_particle_faces_the_camera_unrolled() {
    let mut h = Harness::at_16ms();
    let cam = h.camera(Vec3::new(-14.0, 17.0, 21.0), Vec3::new(1.0, 1.0, 0.0));
    let (priest, _) = h.priest(Vec3::new(1.0, 1.0, 0.0));
    h.app
        .world_mut()
        .get_mut::<Transform>(priest)
        .unwrap()
        .rotation = Quat::from_rotation_y(2.1);
    // A Mind Blast leans the torso back through its loop.
    h.begin_mind_blast(priest);
    h.tick(30);
    let mut rolls = Vec::new();
    for m in h.motes() {
        let (_, rot, _) = m.global.to_scale_rotation_translation();
        let back = cam * Vec3::Z;
        assert!(
            (rot * Vec3::Z).dot(back) > 1.0 - 1e-5,
            "normal {} vs camera back {back}",
            rot * Vec3::Z
        );
        let right = cam * Vec3::X;
        let x = rot * Vec3::X;
        let roll = x.dot(cam * Vec3::Y).atan2(x.dot(right));
        rolls.push(roll);
    }
    assert!(rolls.len() > 10, "{} particles", rolls.len());
    let spread = rolls.iter().cloned().fold(f32::MIN, f32::max)
        - rolls.iter().cloned().fold(f32::MAX, f32::min);
    assert!(rolls.iter().all(|r| r.abs() < 1e-4), "rolls {rolls:?}");
    assert!(spread < 1e-4, "roll spread {spread}");
    // The lean was live, so the counter-rotation was exercised.
    let body = h.app.world().get::<Children>(priest).unwrap()[0];
    let pitch = h.app.world().get::<Transform>(body).unwrap().rotation;
    assert!(
        pitch.angle_between(Quat::IDENTITY) > 0.05,
        "the cast posture leaned the body"
    );
}

// ── the sockets ride the rendered body ─────────────────────────────────────

/// A Priest running at 7 yd/s while it turns, its rendered body bobbing:
/// every frame, each hand sits at its socket on the body AS IT IS DRAWN
/// THAT FRAME, and each particle rides that hand.
#[test]
fn the_hands_ride_the_moving_rendered_body() {
    let mut h = Harness::at_16ms();
    let (priest, body) = h.priest(Vec3::new(-6.0, 1.0, 0.0));
    h.fire(priest, AbilityType::ShadowWordPain);
    let mut checked = 0;
    for f in 0..90 {
        let t = f as f32 * h.dt;
        let caster = Transform::from_xyz(-6.0 + 7.0 * t, 1.0, 0.5 * t)
            .with_rotation(Quat::from_rotation_y(0.8 * t));
        let bob = Transform::from_xyz(0.0, 0.12 * (9.0 * t).sin(), 0.0)
            .with_rotation(Quat::from_rotation_x(0.05 * (5.0 * t).sin()));
        *h.app.world_mut().get_mut::<Transform>(priest).unwrap() = caster;
        *h.app.world_mut().get_mut::<Transform>(body).unwrap() = bob;
        h.tick(1);
        let drawn = caster.compute_matrix()
            * h.app
                .world()
                .get::<Transform>(body)
                .unwrap()
                .compute_matrix();
        let rigs = h.rigs();
        if rigs.is_empty() {
            break;
        }
        for r in &rigs {
            let want = drawn.transform_point3(Vec3::new(SOCKET.x * r.side, SOCKET.y, SOCKET.z));
            assert!(
                r.at.distance(want) < 1e-4,
                "frame {f}: hand {} at {}, the body puts it at {want}",
                r.side,
                r.at
            );
        }
        let rig_at: Vec<(Entity, Vec3)> = rigs.iter().map(|r| (r.entity, r.at)).collect();
        for m in h.motes() {
            let hand = rig_at.iter().find(|(e, _)| *e == m.rig).unwrap().1;
            let off = m.global.translation() - hand;
            let lift = drawn.transform_vector3(Vec3::Y).normalize();
            assert!(
                off.cross(lift).length() < 1e-4,
                "frame {f}: a particle left its hand's up axis: {off}"
            );
            checked += 1;
        }
    }
    assert!(checked > 100, "{checked} particle frames checked");
}

// ── frame-rate independence ────────────────────────────────────────────────

/// What one Shadow Word: Pain draws at `t` seconds, at frame length `tick`:
/// every rig's age and, per hand and emitter, each particle's age and world
/// position in birth order.
fn played_at(tick: Duration, t: f32) -> (Vec<f32>, Vec<Vec<(f32, Vec3)>>) {
    let mut h = Harness::new(tick);
    let (priest, _) = h.priest(Vec3::new(0.0, 1.0, 0.0));
    h.fire(priest, AbilityType::ShadowWordPain);
    let frames = frames_for(t, h.dt);
    h.tick(frames);
    let rigs = h.rigs();
    let mut ages: Vec<f32> = rigs.iter().map(|r| r.age).collect();
    ages.sort_by(|a, b| a.partial_cmp(b).unwrap());
    for age in &ages {
        assert!(
            (age - frames as f32 * h.dt).abs() < 1e-5,
            "rig age {age} after {frames} frames of {}",
            h.dt
        );
    }
    let mut rigs = rigs;
    rigs.sort_by(|a, b| a.side.partial_cmp(&b.side).unwrap());
    let motes = h.motes();
    let mut drawn = Vec::new();
    for r in &rigs {
        for i in 0..3 {
            let mut list: Vec<(f32, Vec3)> = motes
                .iter()
                .filter(|m| m.rig == r.entity && m.emitter == i)
                .map(|m| (m.age, m.global.translation()))
                .collect();
            list.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            drawn.push(list);
        }
    }
    (ages, drawn)
}

/// At 8, 16 and 33 ms frames the same flash draws the same particles, of
/// the same ages, in the same places: inside the kit (0.528 s), past its
/// end (1.056 s) and in its tail (1.584 s). Each count is the clock's.
#[test]
fn the_flash_is_the_same_at_every_frame_rate() {
    for t in [0.528, 1.056, 1.584] {
        let runs: Vec<_> = [8, 16, 33]
            .map(|ms| played_at(Duration::from_millis(ms), t))
            .into_iter()
            .collect();
        let (_, reference) = &runs[0];
        for (hand, list) in reference.iter().enumerate() {
            let e = &EMITTERS[hand % 3];
            let want = live_ages(e, t, KIT_SECS);
            let got: Vec<f32> = list.iter().map(|(a, _)| *a).collect();
            assert_ages(&format!("{t} s, list {hand}"), &got, &want);
        }
        for (ms, (_, drawn)) in [8, 16, 33].iter().zip(&runs) {
            assert_eq!(drawn.len(), reference.len());
            for (list, ref_list) in drawn.iter().zip(reference) {
                assert_eq!(list.len(), ref_list.len(), "{ms} ms at {t} s");
                for ((a, p), (ra, rp)) in list.iter().zip(ref_list) {
                    assert!((a - ra).abs() < 1e-4, "{ms} ms at {t} s: age {a} vs {ra}");
                    assert!(p.distance(*rp) < 1e-4, "{ms} ms at {t} s: {p} vs {rp}");
                }
            }
        }
    }
}

/// The rig lasts its kit and its last particle's life, 2.2 s, at every
/// frame rate: alive a frame before, gone a frame after, with every
/// particle gone with it.
#[test]
fn the_flash_ends_with_its_last_particle_at_every_frame_rate() {
    for ms in [8u64, 16, 33] {
        let mut h = Harness::new(Duration::from_millis(ms));
        let (priest, _) = h.priest(Vec3::new(0.0, 1.0, 0.0));
        h.fire(priest, AbilityType::ShadowWordPain);
        let before = ((RIG_SECS - 0.02) / h.dt).floor() as u32;
        h.tick(before);
        assert_eq!(
            h.rigs().len(),
            2,
            "{ms} ms: alive at {} s",
            before as f32 * h.dt
        );
        let after = ((RIG_SECS + 0.02) / h.dt).ceil() as u32;
        h.tick(after - before);
        assert_eq!(
            h.rigs().len(),
            0,
            "{ms} ms: gone by {} s",
            after as f32 * h.dt
        );
        assert_eq!(
            h.count::<HealCastShadowMote>(),
            0,
            "{ms} ms: no particle outlives its rig"
        );
    }
}

/// A hitch draws only what the clock says is alive. Bevy caps a frame at
/// 250 ms of real time, and the match's top speed (3x) makes that 0.75 s of
/// match time: a ring born a fifth of a second into such a frame is already
/// gone at its end, and is never drawn.
#[test]
fn a_hitch_draws_no_particle_past_its_life() {
    let mut h = Harness::new(Duration::from_millis(250));
    h.app
        .world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_relative_speed(3.0);
    let (priest, _) = h.priest(Vec3::new(0.0, 1.0, 0.0));
    h.fire(priest, AbilityType::ShadowWordPain);
    h.tick(1);
    assert!((h.rigs()[0].age - 0.75).abs() < 1e-5, "one 0.75 s frame");
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 2);
    let motes = h.motes();
    for r in &rigs {
        for (i, e) in EMITTERS.iter().enumerate() {
            assert_ages(
                &format!("hitch, emitter {i}"),
                &ages_of(&motes, r.entity, i),
                &live_ages(e, 0.75, KIT_SECS),
            );
        }
    }
    assert!(
        motes.iter().all(|m| m.age < EMITTERS[m.emitter].life),
        "a particle past its life was drawn"
    );
}

// ── Mind Blast's cast bar ──────────────────────────────────────────────────

/// Mind Blast's cast loops the hands (and no casting orb is drawn): the
/// emitters run from the cast's start; on landing the cast kit plays the
/// model's one more second on the same clock and the posture's release
/// surge is over by then; the rig waits out its last particle, 2.2 s.
#[test]
fn mind_blast_loops_the_hands_then_plays_the_cast_kit() {
    let mut h = Harness::at_16ms();
    let (priest, body) = h.priest(Vec3::new(0.0, 1.0, 0.0));
    h.begin_mind_blast(priest);
    let cast = 1.008; // 63 frames of the 1.5 s cast
    h.tick(frames_for(cast, h.dt));
    assert_eq!(h.count::<CastingOrb>(), 0, "the hands replace the orb");
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 2);
    assert!(rigs
        .iter()
        .all(|r| r.phase == HealCastPhase::Loop && r.kind == HealCastKind::Shadow));
    let motes = h.motes();
    for (i, e) in EMITTERS.iter().enumerate() {
        // The loop has no kit length: everything the clock owes so far.
        assert_ages(
            &format!("loop emitter {i}"),
            &ages_of(&motes, rigs[0].entity, i),
            &live_ages(e, cast, f32::MAX),
        );
    }

    h.end_cast(priest, CastEndingKind::Landed);
    let landed = frames_for(0.528, h.dt);
    h.tick(landed);
    let t = cast + 0.528;
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 2, "a landed cast plays its kit");
    assert!(rigs
        .iter()
        .all(|r| matches!(r.phase, HealCastPhase::Flare { .. })));
    let motes = h.motes();
    for (i, e) in EMITTERS.iter().enumerate() {
        assert_ages(
            &format!("kit emitter {i}"),
            &ages_of(&motes, rigs[0].entity, i),
            &live_ages(e, t, cast + KIT_SECS),
        );
    }

    // Past the kit's second: nothing new is born, and the torso is upright.
    h.tick(frames_for(0.528, h.dt));
    let t = cast + 1.056;
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 2, "the tail plays out");
    let motes = h.motes();
    for (i, e) in EMITTERS.iter().enumerate() {
        assert_ages(
            &format!("tail emitter {i}"),
            &ages_of(&motes, rigs[0].entity, i),
            &live_ages(e, t, cast + KIT_SECS),
        );
    }
    let lean = h.app.world().get::<Transform>(body).unwrap().rotation;
    assert!(
        lean.angle_between(Quat::IDENTITY) < 1e-5,
        "the release is over with the kit: {lean:?}"
    );

    h.tick(frames_for(1.2, h.dt));
    assert_eq!(h.rigs().len(), 0);
    assert_eq!(h.count::<HealCastShadowMote>(), 0);
}

/// An interrupted or fizzled Mind Blast stops the hands dead — every rig
/// and particle gone on the frame — as every hand family's cast does.
#[test]
fn an_interrupted_mind_blast_stops_the_hands_dead() {
    for kind in [CastEndingKind::Interrupted, CastEndingKind::Fizzled] {
        let mut h = Harness::at_16ms();
        let (priest, _) = h.priest(Vec3::new(0.0, 1.0, 0.0));
        h.begin_mind_blast(priest);
        h.tick(40);
        assert_eq!(h.rigs().len(), 2);
        assert!(h.count::<HealCastShadowMote>() > 10);
        h.end_cast(priest, kind);
        h.tick(1);
        assert_eq!(h.rigs().len(), 0, "{kind:?}");
        assert_eq!(h.count::<HealCastShadowMote>(), 0, "{kind:?}");
    }
}

// ── lifecycle ──────────────────────────────────────────────────────────────

/// A Priest wearing what it commonly wears (Shadowform's compound, a Power
/// Word: Shield, Fortitude), half a second into a Shadow Word: Pain.
fn dressed_flash() -> (Harness, Entity) {
    let mut h = Harness::at_16ms();
    let (priest, _) = h.priest(Vec3::new(0.0, 1.0, 0.0));
    let defs = AbilityDefinitions::default();
    for ability in [
        AbilityType::Shadowform,
        AbilityType::PowerWordShield,
        AbilityType::PowerWordFortitude,
    ] {
        let aura = AuraPending::from_ability(priest, priest, defs.get_unchecked(&ability))
            .unwrap()
            .aura;
        h.app
            .world_mut()
            .get_mut::<ActiveAuras>(priest)
            .unwrap()
            .auras
            .push(aura);
    }
    h.fire(priest, AbilityType::ShadowWordPain);
    h.tick(30);
    assert_eq!(h.rigs().len(), 2);
    assert!(h.count::<HealCastShadowMote>() > 0);
    (h, priest)
}

/// The hands outlive the cast by up to 2.2 s, so they go with their caster:
/// on the frame it dies, is polymorphed (the sheep has no hands) or is
/// despawned, and at the match's end with every `PlayMatchEntity`. The
/// control, the same Priest left alone, keeps them.
#[test]
fn the_hands_go_with_their_caster() {
    let (mut h, _) = dressed_flash();
    h.tick(1);
    assert_eq!(h.rigs().len(), 2, "the control keeps its hands");

    let (mut h, priest_d) = dressed_flash();
    h.app
        .world_mut()
        .get_mut::<Combatant>(priest_d)
        .unwrap()
        .current_health = 0.0;
    h.tick(1);
    assert_eq!(h.rigs().len(), 0, "dead");
    assert_eq!(h.count::<HealCastShadowMote>(), 0, "dead");

    let (mut h, priest_s) = dressed_flash();
    h.app
        .world_mut()
        .entity_mut(priest_s)
        .insert(PolymorphedVisual);
    // A sheep's aura beside the others, as the sim lands it.
    h.app
        .world_mut()
        .get_mut::<ActiveAuras>(priest_s)
        .unwrap()
        .auras
        .push(Aura {
            effect_type: AuraType::Polymorph,
            duration: 8.0,
            ability_name: "Polymorph".into(),
            ..Default::default()
        });
    h.tick(1);
    assert_eq!(h.rigs().len(), 0, "sheeped");
    assert_eq!(h.count::<HealCastShadowMote>(), 0, "sheeped");

    let (mut h, priest_x) = dressed_flash();
    h.app.world_mut().entity_mut(priest_x).despawn();
    h.tick(1);
    assert_eq!(h.rigs().len(), 0, "despawned");
    assert_eq!(h.count::<HealCastShadowMote>(), 0, "despawned");

    let (mut h, _) = dressed_flash();
    h.app
        .world_mut()
        .run_system_once(cleanup_play_match)
        .unwrap();
    h.tick(1);
    assert_eq!(h.rigs().len(), 0, "match end");
    assert_eq!(h.count::<HealCastShadowMote>(), 0, "match end");
}

/// The hands leave the body they ride alone: its material, mesh and
/// visibility (Shadowform's look edits the material in place; stealth reads
/// its alpha) are what they were.
#[test]
fn the_hands_leave_the_body_alone() {
    let mut h = Harness::at_16ms();
    let (priest, body) = h.priest(Vec3::new(0.0, 1.0, 0.0));
    let material = h
        .app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgba(0.15, 0.0, 0.26, 0.65),
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
    h.app
        .world_mut()
        .entity_mut(body)
        .insert((MeshMaterial3d(material.clone()), Visibility::Inherited));
    let before = h
        .app
        .world()
        .resource::<Assets<StandardMaterial>>()
        .get(&material)
        .unwrap()
        .base_color;
    h.fire(priest, AbilityType::Silence);
    h.tick(60);
    let world = h.app.world();
    assert_eq!(
        world
            .get::<MeshMaterial3d<StandardMaterial>>(body)
            .unwrap()
            .0,
        material
    );
    assert_eq!(
        *world.get::<Visibility>(body).unwrap(),
        Visibility::Inherited
    );
    assert_eq!(
        world
            .resource::<Assets<StandardMaterial>>()
            .get(&material)
            .unwrap()
            .base_color,
        before
    );
}

// ── the real Priest decisions ──────────────────────────────────────────────

const PRIEST: &str = "priest";
const MAGE: &str = "enemy mage";
const HEALER: &str = "enemy priest";

fn fortitude() -> Aura {
    Aura {
        effect_type: AuraType::MaxHealthIncrease,
        duration: 600.0,
        ability_name: "Power Word: Fortitude".into(),
        ..Default::default()
    }
}

/// A Shadowform Priest with Vampiric Embrace and Silence, topped, its kill
/// target an enemy Mage 20 yards off; an enemy Priest 16 yards off.
fn shadow_scene() -> Scene {
    Scene::new()
        .unit(PRIEST, 1, CharacterClass::Priest)
        .targeting(MAGE)
        .aura_full(PRIEST, fortitude())
        .aura(PRIEST, AuraType::WeakenedSoul, 15.0)
        .unit(MAGE, 2, CharacterClass::Mage)
        .at(20.0, 0.0)
        .unit(HEALER, 2, CharacterClass::Priest)
        .at(16.0, 3.0)
        .in_shadowform(PRIEST)
        .grant(PRIEST, AbilityType::VampiricEmbrace)
        .grant(PRIEST, AbilityType::Silence)
}

/// `scene` with the Priest's own `ability` aura already on the Mage.
fn already_on_mage(scene: Scene, ability: AbilityType) -> Scene {
    let def = AbilityDefinitions::shipped()
        .get_unchecked(&ability)
        .clone();
    let aura = AuraPending::from_ability(scene.entity(MAGE), scene.entity(PRIEST), &def)
        .unwrap()
        .aura;
    scene.aura_full(MAGE, aura)
}

/// Replays a decision's markers through the real hand system against a
/// rendered stand-in for the Priest: the Shadow rigs they grew.
fn hands_from(view: &DecisionView) -> Vec<RigView> {
    let mut h = Harness::at_16ms();
    let (priest, _) = h.priest(Vec3::new(0.0, 1.0, 0.0));
    for m in &view.instant_markers {
        assert_eq!(m.caster, PRIEST, "a marker named another caster");
        h.fire(priest, m.ability);
    }
    h.tick(1);
    h.rigs()
}

fn shadow_markers(view: &DecisionView) -> Vec<AbilityType> {
    view.instant_markers
        .iter()
        .map(|m| m.ability)
        .filter(|a| HealCastKind::for_ability(*a) == Some(HealCastKind::Shadow))
        .collect()
}

/// The real Priest AI's three Shadow instants each spawn exactly one marker
/// naming the instant and its target, and that marker lights the Priest's
/// two hands, once. Mind Blast, a cast, spawns none (its cast bar plays the
/// hands).
#[test]
fn each_shadow_instant_the_priest_casts_lights_its_hands_once() {
    let cases: [(Scene, AbilityType, &str); 3] = [
        (shadow_scene(), AbilityType::ShadowWordPain, MAGE),
        (
            already_on_mage(shadow_scene(), AbilityType::ShadowWordPain),
            AbilityType::VampiricEmbrace,
            MAGE,
        ),
        (
            shadow_scene().casting(HEALER, AbilityType::FlashHeal),
            AbilityType::Silence,
            HEALER,
        ),
    ];
    for (scene, ability, on) in cases {
        let view = scene.decide(PRIEST);
        assert_eq!(
            view.chose(),
            Some((ability, Some(on))),
            "{:?}",
            view.choices
        );
        assert_eq!(
            view.instant_markers.len(),
            1,
            "{ability:?}: {:?}",
            view.instant_markers
        );
        let marker = view.instant_markers[0];
        assert_eq!(
            (marker.caster, marker.target, marker.ability),
            (PRIEST, Some(on), ability)
        );
        let rigs = hands_from(&view);
        assert_eq!(rigs.len(), 2, "{ability:?} lights both hands once");
        assert!(rigs.iter().all(|r| r.kind == HealCastKind::Shadow));
    }
    let blast = already_on_mage(
        already_on_mage(shadow_scene(), AbilityType::ShadowWordPain),
        AbilityType::VampiricEmbrace,
    )
    .decide(PRIEST);
    assert_eq!(blast.chose(), Some((AbilityType::MindBlast, Some(MAGE))));
    assert!(
        blast.instant_markers.is_empty(),
        "a cast's hands are its cast bar's"
    );
}

/// A silenced Priest casts none of them, and a polymorphed one decides
/// nothing at all: no marker, no hands.
#[test]
fn a_silenced_or_sheeped_priest_lights_nothing() {
    let silenced = shadow_scene()
        .casting(HEALER, AbilityType::FlashHeal)
        .aura(PRIEST, AuraType::Silence, 5.0)
        .decide(PRIEST);
    assert!(
        shadow_markers(&silenced).is_empty(),
        "{:?}",
        silenced.instant_markers
    );
    assert!(hands_from(&silenced).is_empty());
    assert!(silenced
        .choices
        .iter()
        .all(|(a, _)| HealCastKind::for_ability(*a) != Some(HealCastKind::Shadow)));

    let sheeped = shadow_scene()
        .casting(HEALER, AbilityType::FlashHeal)
        .aura(PRIEST, AuraType::Polymorph, 8.0)
        .decide_held(PRIEST);
    assert!(
        sheeped.instant_markers.is_empty(),
        "{:?}",
        sheeped.instant_markers
    );
    assert!(hands_from(&sheeped).is_empty());
}
