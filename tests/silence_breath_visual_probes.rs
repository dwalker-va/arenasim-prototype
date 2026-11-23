//! Probes for Priest Silence's state (AS-357): `shadowwordsilence_breath.m2`
//! at the victim's mouth for as long as the silence holds
//! (`rendering/effects/silence_breath.rs`) — the inhale on its onset, then an
//! exhale each 4.167 s loop — and for the route that gives Priest Silence its
//! breath while Unstable Affliction's backlash silence keeps its burst.
//!
//! Everything here asserts what is DRAWN — world positions under propagated
//! `GlobalTransform`s, the materials' colours, alphas, emissives and blend
//! modes, the sprites' textures, scales and rolls — against the signed-off
//! AS-335 Priest Talent Visuals Bench (version 1792207490-53c9, "Silence"
//! tab), transcribed below straight from
//! `docs/design/benches/2026-10-17-priest-talent-bench.html` and NOT from the
//! module's tables, so a wrong value in the module cannot agree with itself
//! here. (A particle's layer is identified by its emitter's name, and nothing
//! else is read from the module's table.)
//!
//! The silence comes through the sim: a zero-length cast resolved by the real
//! `process_casting`, its aura landed by the real `apply_pending_auras` and
//! run down by the real `update_auras`. Runs on `MinimalPlugins` +
//! `AssetPlugin` + `TransformPlugin` with the chain `states/mod.rs`
//! registers.

use std::collections::BTreeSet;
use std::f32::consts::{PI, TAU};
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::auras::{apply_pending_auras, update_auras};
use arenasim::states::play_match::combat_core::process_casting;
use arenasim::states::play_match::components::{
    is_silence_breath, ActiveAuras, ArenaDampening, Aura, AuraApplyOwner, AuraApplyRoute, AuraType,
    CastingState, Combatant, GameRng, MatchCountdown, Pet, PetType, PlayMatchEntity,
    PolymorphedVisual, VisualBody,
};
use arenasim::states::play_match::dispel_backlash_silence_aura;
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::{
    age_silence_breath_particles, animate_silence_breaths, cleanup_play_match,
    cleanup_silence_breaths, place_silence_breaths, spawn_silence_breaths, SilenceBreathParticle,
    SilenceBreathRig, SILENCE_EMITTERS,
};
use arenasim::CharacterClass;

const DT: f32 = 0.016;
/// Combatants stand with capsule centres at world y = 1.0 over the floor.
const COMBATANT_Y: f32 = 1.0;
/// The combatant body mesh, `Capsule3d::new(0.5, 1.5)`.
const BODY_RADIUS: f32 = 0.5;
const BODY_HALF_SEGMENT: f32 = 0.75;

// ── The bench, transcribed ───────────────────────────────────────────────────

/// `anchor:'head'` (`ANCH.head` 2.25) and `pos:[0,0.3,-0.15]`: the emitter's
/// `spawn` puts `pos[1]` on z (0.3, the face) and `pos[2]` on y (-0.15).
const BENCH_HEAD: f32 = 2.25;
const MOUTH_DOWN: f32 = 0.15;
const MOUTH_FORWARD: f32 = 0.3;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Launch {
    /// `inward: shell` — born 0.6-1.0 of it out, travelling straight in.
    Inward(f32),
    /// `vRange` below 3.1: a polar angle up to it off straight up.
    Cone(f32),
    /// `vRange >= 3.1`: every direction.
    Sphere,
}

struct BenchLayer {
    /// The module's emitter it must be (identification only).
    name: &'static str,
    /// `Emitter.draw`: `'lighter'` unless the layer says `blend:'alpha'`.
    additive: bool,
    launch: Launch,
    speed: f32,
    life: f32,
    rate: f32,
    /// `win`, in ms from the onset, as the bench plays its first 5 s.
    windows: &'static [(f32, f32)],
    color: &'static [(f32, [f32; 3])],
    alpha: &'static [(f32, f32)],
    scale: &'static [(f32, f32)],
    /// `spin`, turns a second.
    spin: f32,
    /// `sprite`'s gradient stops for its `kind` (`glow` [1, 0.45], `smoke`
    /// [0.85, 0.5]); `None` for a `star`.
    falloff: Option<[(f32, f32); 3]>,
}

const INHALE: usize = 0;
const INSMOKE: usize = 1;
const EXHALE: usize = 2;
const SPARKLE: usize = 3;

const BENCH: [BenchLayer; 4] = [
    // {id:'inhale', ... inward:0.75, speed:-3.3, life:0.25, rate:240,
    //  win:[[0,1],[167,0]], ... kind:'glow', follow:true}
    BenchLayer {
        name: "shadowwordsilence_breath/inhale motes",
        additive: true,
        launch: Launch::Inward(0.75),
        speed: 3.3,
        life: 0.25,
        rate: 240.0,
        windows: &[(0.0, 167.0)],
        color: &[
            (0.0, [150.0, 60.0, 255.0]),
            (0.5, [255.0, 40.0, 200.0]),
            (1.0, [60.0, 220.0, 120.0]),
        ],
        alpha: &[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.02), (1.0, 0.035)],
        spin: 0.0,
        falloff: Some([(0.0, 1.0), (0.45, 0.45), (1.0, 0.0)]),
    },
    // {id:'insmoke', ... inward:0.6, speed:-0.83, life:0.6, rate:100,
    //  win:[[0,0],[233,1],[867,0]], blend:'alpha', ... kind:'smoke'}
    BenchLayer {
        name: "shadowwordsilence_breath/inhaled smoke",
        additive: false,
        launch: Launch::Inward(0.6),
        speed: 0.83,
        life: 0.6,
        rate: 100.0,
        windows: &[(233.0, 867.0)],
        color: &[(0.0, [120.0, 120.0, 120.0]), (1.0, [163.0, 0.0, 175.0])],
        alpha: &[(0.0, 0.7), (1.0, 0.0)],
        scale: &[(0.0, 0.56), (1.0, 0.03)],
        spin: 0.0,
        falloff: Some([(0.0, 0.85), (0.45, 0.5), (1.0, 0.0)]),
    },
    // {id:'exhale', ... vRange:0.8, speed:0.28, life:1.5, rate:100,
    //  win:[[0,0],[1400,1],[1967,0]], blend:'alpha', ... kind:'smoke'}
    BenchLayer {
        name: "shadowwordsilence_breath/exhaled smoke",
        additive: false,
        launch: Launch::Cone(0.8),
        speed: 0.28,
        life: 1.5,
        rate: 100.0,
        windows: &[(1400.0, 1967.0)],
        color: &[
            (0.0, [130.0, 130.0, 130.0]),
            (0.5, [163.0, 0.0, 175.0]),
            (1.0, [160.0, 190.0, 240.0]),
        ],
        alpha: &[(0.0, 0.8), (1.0, 0.0)],
        scale: &[(0.0, 0.03), (0.5, 0.08), (1.0, 0.28)],
        spin: 0.0,
        falloff: Some([(0.0, 0.85), (0.45, 0.5), (1.0, 0.0)]),
    },
    // {id:'sparkle', ... vRange:3.14, speed:0.4, spin:1.1, life:0.8, rate:60,
    //  win:[[0,0],[233,1],[867,0],[1534,1],[2334,0]], ... kind:'star'}
    BenchLayer {
        name: "shadowwordsilence_breath/sparkles",
        additive: true,
        launch: Launch::Sphere,
        speed: 0.4,
        life: 0.8,
        rate: 60.0,
        windows: &[(233.0, 867.0), (1534.0, 2334.0)],
        color: &[
            (0.0, [60.0, 220.0, 120.0]),
            (0.5, [255.0, 40.0, 200.0]),
            (1.0, [30.0, 40.0, 160.0]),
        ],
        alpha: &[(0.0, 1.0), (1.0, 0.0)],
        scale: &[(0.0, 0.05), (1.0, 0.03)],
        spin: 1.1,
        falloff: None,
    },
];

/// The client's 158 sequence: the loop the exhale and its sparkles repeat on,
/// from the end of the 1167 ms Stand (research §8).
const LOOP_MS: f32 = 4167.0;

/// The bench's `keys`: linear between keys, clamped outside them.
fn bench_keys(k: &[(f32, f32)], t: f32) -> f32 {
    if t <= k[0].0 {
        return k[0].1;
    }
    for w in k.windows(2) {
        if t <= w[1].0 {
            let span = w[1].0 - w[0].0;
            let u = if span > 0.0 { (t - w[0].0) / span } else { 1.0 };
            return w[0].1 + (w[1].1 - w[0].1) * u;
        }
    }
    k[k.len() - 1].1
}

fn bench_rgb(k: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    let ch = |c: usize| {
        let ks: Vec<(f32, f32)> = k.iter().map(|(t, v)| (*t, v[c])).collect();
        bench_keys(&ks, t)
    };
    [ch(0), ch(1), ch(2)]
}

fn layer_index(emitter: usize) -> usize {
    let name = SILENCE_EMITTERS[emitter].name;
    BENCH
        .iter()
        .position(|l| l.name == name)
        .unwrap_or_else(|| panic!("no bench layer {name}"))
}

/// A drawn material against a bench colour, alpha and blend.
fn assert_drawn(m: &StandardMaterial, additive: bool, rgb: [f32; 3], alpha: f32, what: &str) {
    let s = m.base_color.to_srgba();
    let got = [s.red * 255.0, s.green * 255.0, s.blue * 255.0];
    for k in 0..3 {
        assert!(
            (got[k] - rgb[k]).abs() < 0.01,
            "{what}: drawn {got:?}, bench {rgb:?}"
        );
    }
    assert!(
        (m.base_color.alpha() - alpha).abs() < 1e-4,
        "{what}: alpha {} against the bench's {alpha}",
        m.base_color.alpha()
    );
    if additive {
        assert_eq!(
            m.alpha_mode,
            AlphaMode::Add,
            "{what}: 'lighter' is additive"
        );
        let lin = Color::srgb(rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0).to_linear();
        let e = m.emissive;
        for (got, want) in [
            (e.red, lin.red * 2.5),
            (e.green, lin.green * 2.5),
            (e.blue, lin.blue * 2.5),
        ] {
            assert!((got - want).abs() < 1e-4, "{what}: emissive {e:?}");
        }
    } else {
        assert_eq!(
            m.alpha_mode,
            AlphaMode::Blend,
            "{what}: source-over is blended"
        );
        assert!(m.unlit, "{what}: a blended piece is a flat colour");
    }
}

// ── The harness ──────────────────────────────────────────────────────────────

struct Harness {
    app: App,
    /// The frame's length, seconds.
    dt: f32,
}

impl Harness {
    fn new() -> Self {
        Harness::with_tick(DT)
    }

    /// A harness whose frames are `dt` seconds long.
    fn with_tick(dt: f32) -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::transform::TransformPlugin,
        ));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            dt,
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
            bounds: Default::default(),
            volumes: Vec::new(),
            cover_anchors: Vec::new(),
        });
        // The sim's cast, its aura's landing and its running down, then the
        // chain `states/mod.rs` registers.
        app.add_systems(
            Update,
            (update_auras, process_casting, apply_pending_auras).chain(),
        );
        app.add_systems(
            Update,
            (
                spawn_silence_breaths,
                animate_silence_breaths,
                age_silence_breath_particles,
                cleanup_silence_breaths,
            )
                .chain()
                .after(apply_pending_auras),
        );
        // Posed on the rendered body in PostUpdate, before propagation, as
        // `states/mod.rs` registers it.
        app.add_systems(
            PostUpdate,
            place_silence_breaths.before(bevy::transform::TransformSystem::TransformPropagate),
        );
        // Warm the clock: Bevy's first update has a zero delta.
        app.update();
        Harness { app, dt }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn tick_secs(&mut self, secs: f32) {
        self.tick((secs / self.dt).round() as u32);
    }

    fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }

    fn camera(&mut self, transform: Transform) {
        self.world().spawn((Camera3d::default(), transform));
    }

    fn priest(&mut self, at: Vec3) -> Entity {
        let defs = AbilityDefinitions::default();
        let mut c = Combatant::new(1, 0, CharacterClass::Priest);
        let book = c.book().clone().granting(&defs, AbilityType::Silence);
        c = c.with_book(book);
        c.max_mana = 5000.0;
        c.current_mana = 5000.0;
        self.world()
            .spawn((c, Transform::from_translation(at)))
            .id()
    }

    /// An enemy Mage at `at`, facing `yaw` (the sim's facing: local +Z turned
    /// about Y).
    fn mage(&mut self, at: Vec3, yaw: f32) -> Entity {
        self.world()
            .spawn((
                Combatant::new(2, 0, CharacterClass::Mage),
                Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)),
            ))
            .id()
    }

    /// Start a zero-length cast and run frames until it has landed.
    fn cast(&mut self, caster: Entity, target: Entity, ability: AbilityType) {
        self.world()
            .entity_mut(caster)
            .insert(CastingState::new(ability, target, 0.0));
        for _ in 0..5 {
            self.tick(1);
            if self.app.world().get::<CastingState>(caster).is_none() {
                return;
            }
        }
        panic!("{ability:?} never landed");
    }

    fn rigs(&mut self) -> Vec<(Entity, Entity, f32, Vec3)> {
        let mut q = self
            .world()
            .query::<(Entity, &SilenceBreathRig, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(e, r, g)| (e, r.target, r.age, g.translation()))
            .collect()
    }

    fn particles(&mut self) -> Vec<(Entity, usize, f32, f32, Vec3, Vec3, f32)> {
        let mut q = self.world().query::<(Entity, &SilenceBreathParticle)>();
        q.iter(self.app.world())
            .map(|(e, p)| (e, p.emitter, p.age, p.life, p.offset, p.velocity, p.angle))
            .collect()
    }

    fn material(&self, entity: Entity) -> StandardMaterial {
        let world = self.app.world();
        world
            .resource::<Assets<StandardMaterial>>()
            .get(
                &world
                    .get::<MeshMaterial3d<StandardMaterial>>(entity)
                    .unwrap()
                    .0,
            )
            .unwrap()
            .clone()
    }

    fn texel(&self, m: &StandardMaterial, x: u32, y: u32) -> (f32, f32) {
        let images = self.app.world().resource::<Assets<Image>>();
        let image = images
            .get(&m.base_color_texture.clone().expect("a sprite texture"))
            .unwrap();
        let w = image.width();
        let data = image.data.as_ref().unwrap();
        let dx = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
        let dy = (y as f32 + 0.5) / w as f32 * 2.0 - 1.0;
        (
            (dx * dx + dy * dy).sqrt(),
            data[((y * w + x) * 4 + 3) as usize] as f32 / 255.0,
        )
    }

    fn width(&self, m: &StandardMaterial) -> u32 {
        let images = self.app.world().resource::<Assets<Image>>();
        images
            .get(&m.base_color_texture.clone().unwrap())
            .unwrap()
            .width()
    }
}

/// The mouth of a unit standing at `at` facing `yaw`, as the bench places it.
fn mouth(at: Vec3, yaw: f32) -> Vec3 {
    at + Vec3::Y * (BENCH_HEAD - MOUTH_DOWN - COMBATANT_Y)
        + Vec3::new(yaw.sin(), 0.0, yaw.cos()) * MOUTH_FORWARD
}

fn match_camera(target: Vec3, zoom: f32, pitch: f32, yaw: f32) -> Transform {
    let at = target
        + Vec3::new(
            zoom * pitch.sin() * yaw.sin(),
            zoom * pitch.cos(),
            zoom * pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

fn body_nearest_depth(at: Vec3, eye: Vec3, forward: Vec3) -> f32 {
    [
        at - Vec3::Y * BODY_HALF_SEGMENT,
        at + Vec3::Y * BODY_HALF_SEGMENT,
    ]
    .iter()
    .map(|e| (*e - eye).dot(forward))
    .fold(f32::MAX, f32::min)
        - BODY_RADIUS
}

const MAGE_AT: Vec3 = Vec3::new(4.0, COMBATANT_Y, -1.0);
const MAGE_YAW: f32 = 1.1;

/// A Priest that has just silenced a Mage facing [`MAGE_YAW`].
fn silenced() -> (Harness, Entity, Entity) {
    silenced_at(DT)
}

/// [`silenced`], on frames `dt` long.
fn silenced_at(dt: f32) -> (Harness, Entity, Entity) {
    let mut h = Harness::with_tick(dt);
    let priest = h.priest(Vec3::new(-8.0, COMBATANT_Y, 0.0));
    let mage = h.mage(MAGE_AT, MAGE_YAW);
    h.cast(priest, mage, AbilityType::Silence);
    (h, priest, mage)
}

// ── Routing ──────────────────────────────────────────────────────────────────

/// Priest Silence's application routes to its breath, by name; the Silence
/// TYPE — and so Unstable Affliction's backlash silence, named after the DoT —
/// still routes to the backlash burst, as before.
#[test]
fn priest_silence_routes_to_its_breath_and_the_backlash_keeps_its_burst() {
    assert_eq!(
        AuraApplyRoute::for_instance(AuraType::Silence, "Silence"),
        AuraApplyRoute::Owned(AuraApplyOwner::SilenceBreath)
    );
    let backlash = dispel_backlash_silence_aura(None, 5.0);
    assert_eq!(backlash.effect_type, AuraType::Silence);
    assert_eq!(
        AuraApplyRoute::for_instance(backlash.effect_type, &backlash.ability_name),
        AuraApplyRoute::Owned(AuraApplyOwner::BacklashBurst)
    );
    assert_eq!(
        AuraApplyRoute::for_aura(AuraType::Silence),
        AuraApplyRoute::Owned(AuraApplyOwner::BacklashBurst)
    );
    assert!(is_silence_breath(AuraType::Silence, "Silence"));
    assert!(!is_silence_breath(AuraType::Silence, "Unstable Affliction"));
    assert!(!is_silence_breath(AuraType::SpellSchoolLockout, "Silence"));
    let defs = AbilityDefinitions::default();
    let silence = defs.get_unchecked(&AbilityType::Silence);
    assert_eq!(silence.name, "Silence");
    assert_eq!(
        silence.applies_aura.as_ref().unwrap().aura_type,
        AuraType::Silence
    );
}

// ── Through the sim ──────────────────────────────────────────────────────────

/// The real cast lands one breath, on its target, at its mouth — the head
/// anchor, dropped 0.15 and brought 0.3 forward along the way the victim faces
/// — beside a Shadow Word: Pain already on it. Its control: the backlash's
/// silence, landed on another enemy, raises none.
#[test]
fn the_cast_raises_one_breath_at_its_targets_mouth_and_the_backlash_none() {
    let mut h = Harness::new();
    let priest = h.priest(Vec3::new(-8.0, COMBATANT_Y, 0.0));
    let mage = h.mage(MAGE_AT, MAGE_YAW);
    let dispeller = h.mage(Vec3::new(2.0, COMBATANT_Y, 5.0), 0.3);
    h.world().get_mut::<Combatant>(dispeller).unwrap().slot = 1;
    h.cast(priest, mage, AbilityType::ShadowWordPain);
    h.world().entity_mut(dispeller).insert(ActiveAuras {
        auras: vec![dispel_backlash_silence_aura(Some(priest), 5.0)],
    });
    h.tick(3);
    assert!(h.rigs().is_empty(), "the backlash's silence has no breath");

    h.cast(priest, mage, AbilityType::Silence);
    let auras = h.app.world().get::<ActiveAuras>(mage).unwrap();
    assert_eq!(auras.auras.len(), 2, "beside the Shadow Word: Pain");
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1, "one breath");
    let (rig, target, _, at) = rigs[0];
    assert_eq!(target, mage, "on the silenced unit");
    assert!(
        (at - mouth(MAGE_AT, MAGE_YAW)).length() < 1e-4,
        "at the mouth: {at} vs {}",
        mouth(MAGE_AT, MAGE_YAW)
    );
    assert!(h.app.world().get::<PlayMatchEntity>(rig).is_some());

    // The mouth turns with the victim.
    let yaw = MAGE_YAW + PI;
    h.world().get_mut::<Transform>(mage).unwrap().rotation = Quat::from_rotation_y(yaw);
    h.tick(1);
    let (_, _, _, at) = h.rigs()[0];
    assert!((at - mouth(MAGE_AT, yaw)).length() < 1e-4, "turned: {at}");
}

// ── The layers ───────────────────────────────────────────────────────────────

/// One particle's birth, read the frame it first appears (one frame old).
struct Birth {
    layer: usize,
    ms: f32,
    born: Vec3,
    launch: Vec3,
}

/// Every birth of a breath, until `until_ms` after its onset, with the
/// silence refreshed to its full 5 s at each of `refresh_at_ms`, on frames
/// `dt` long.
fn births(until_ms: f32, refresh_at_ms: &[f32], dt: f32) -> Vec<Birth> {
    let (mut h, _, mage) = silenced_at(dt);
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let mut refreshes = refresh_at_ms.to_vec();
    loop {
        let rigs = h.rigs();
        assert_eq!(rigs.len(), 1, "one breath, held");
        let ms = rigs[0].2 * 1000.0;
        if ms > until_ms {
            break;
        }
        if refreshes.first().is_some_and(|r| ms >= *r) {
            refreshes.remove(0);
            let mut auras = h.world().get_mut::<ActiveAuras>(mage).unwrap();
            let silence = auras
                .auras
                .iter_mut()
                .find(|a| a.effect_type == AuraType::Silence)
                .unwrap();
            silence.duration = 5.0;
        }
        for (e, emitter, age, life, offset, velocity, _) in h.particles() {
            if !seen.insert(e) {
                continue;
            }
            let layer = layer_index(emitter);
            assert!((age - dt).abs() < 1e-5, "first seen one frame old");
            assert!((life - BENCH[layer].life).abs() < 1e-6);
            out.push(Birth {
                layer,
                ms,
                born: offset - velocity * dt,
                launch: velocity,
            });
        }
        h.tick(1);
    }
    out
}

fn in_windows(windows: &[(f32, f32)], ms: f32, dt: f32) -> bool {
    // A birth is recorded on the frame it is emitted, which may close the
    // window by a frame.
    windows
        .iter()
        .any(|(a, b)| ms >= *a && ms < *b + dt * 1000.0 + 0.01)
}

/// Each layer emits only inside its own windows, at its rate, launched as the
/// bench launches it: the inhale and the inhaled smoke born on their shells
/// (filling 0.6-1.0 of them) and travelling straight in, the exhale breathed
/// out of the mouth within its 0.8 rad cone (filling it), the sparkles out in
/// every direction. The same at 8 ms and 33 ms frames as at 16: every rate,
/// age and flight is per second, never per frame.
#[test]
fn each_layer_emits_in_its_windows_as_the_bench_launches_it() {
    for dt in [DT, 0.008, 0.033] {
        layers_emit_as_the_bench_launches_them(dt);
    }
}

fn layers_emit_as_the_bench_launches_them(dt: f32) {
    let births = births(5000.0, &[], dt);
    for (index, layer) in BENCH.iter().enumerate() {
        let mine: Vec<&Birth> = births.iter().filter(|b| b.layer == index).collect();
        let expected: f32 = layer
            .windows
            .iter()
            .map(|(a, b)| layer.rate * (b - a) / 1000.0)
            .sum();
        // Each window may gain or lose a frame's worth at its edges.
        let slack = |windows: usize| windows as f32 * (layer.rate * dt + 1.0);
        assert!(
            (mine.len() as f32 - expected).abs() <= slack(layer.windows.len()),
            "{}: {} births against the bench's {expected}",
            layer.name,
            mine.len()
        );
        for (a, b) in layer.windows {
            let n = mine
                .iter()
                .filter(|x| x.ms >= *a && x.ms <= *b + 17.0)
                .count() as f32;
            let want = layer.rate * (b - a) / 1000.0;
            assert!(
                (n - want).abs() <= slack(1),
                "{}: {n} in [{a}, {b}) against {want}",
                layer.name
            );
        }
        let (mut lo, mut hi) = (f32::MAX, 0.0_f32);
        // The half-spaces the births come from: (axis, side).
        let mut sides = BTreeSet::new();
        for b in &mine {
            assert!(
                in_windows(layer.windows, b.ms, dt),
                "{}: born at {} ms",
                layer.name,
                b.ms
            );
            assert!(
                (b.launch.length() - layer.speed).abs() < 1e-3,
                "{}: speed {}",
                layer.name,
                b.launch.length()
            );
            match layer.launch {
                Launch::Inward(shell) => {
                    let r = b.born.length();
                    assert!(
                        (0.6 * shell - 1e-4..=shell + 1e-4).contains(&r),
                        "{}: born {r} out",
                        layer.name
                    );
                    lo = lo.min(r / shell);
                    hi = hi.max(r / shell);
                    assert!(
                        b.launch.normalize().dot(-b.born.normalize()) > 0.9999,
                        "{}: travels straight in",
                        layer.name
                    );
                }
                Launch::Cone(half) => {
                    assert!(b.born.length() < 1e-4, "{}: out of the mouth", layer.name);
                    let tilt = b.launch.normalize().y.clamp(-1.0, 1.0).acos();
                    assert!(tilt <= half + 1e-4, "{}: {tilt} off up", layer.name);
                    lo = lo.min(tilt / half);
                    hi = hi.max(tilt / half);
                }
                Launch::Sphere => {
                    assert!(b.born.length() < 1e-4, "{}: out of the mouth", layer.name);
                }
            }
            let d = if b.born.length() > 1e-4 {
                b.born
            } else {
                b.launch
            };
            for (axis, v) in [d.x, d.y, d.z].into_iter().enumerate() {
                sides.insert((axis, v > 0.0));
            }
        }
        match layer.launch {
            Launch::Inward(_) => assert!(lo < 0.65 && hi > 0.95, "{}: {lo}..{hi}", layer.name),
            Launch::Cone(_) => {
                assert!(lo < 0.15 && hi > 0.85, "{}: {lo}..{hi}", layer.name);
                assert_eq!(
                    sides,
                    BTreeSet::from([(0, false), (0, true), (1, true), (2, false), (2, true)]),
                    "{}: every bearing, and only upward",
                    layer.name
                );
            }
            Launch::Sphere => {}
        }
        if !matches!(layer.launch, Launch::Cone(_)) {
            assert_eq!(sides.len(), 6, "{}: from every side", layer.name);
        }
    }
}

/// The 158 loop repeats every 4.167 s from the end of the Stand: a silence
/// refreshed before it ends keeps its breath (no second inhale) and exhales —
/// with its sparkles — again 4.167 s after the first, and not in between.
#[test]
fn a_refreshed_silence_exhales_again_one_loop_later_and_never_inhales_again() {
    let births = births(7000.0, &[4000.0], DT);
    let of = |layer: usize| -> Vec<f32> {
        births
            .iter()
            .filter(|b| b.layer == layer)
            .map(|b| b.ms)
            .collect()
    };
    let exhale = of(EXHALE);
    let (first, second): (Vec<f32>, Vec<f32>) = exhale.iter().partition(|ms| **ms < 3000.0);
    let window = BENCH[EXHALE].windows[0];
    for ms in &first {
        assert!(in_windows(&[window], *ms, DT), "first exhale at {ms}");
    }
    let looped = (window.0 + LOOP_MS, window.1 + LOOP_MS);
    for ms in &second {
        assert!(in_windows(&[looped], *ms, DT), "second exhale at {ms}");
    }
    let per = BENCH[EXHALE].rate * (window.1 - window.0) / 1000.0;
    let slack = BENCH[EXHALE].rate * DT + 1.0;
    assert!((first.len() as f32 - per).abs() <= slack, "{}", first.len());
    assert!(
        (second.len() as f32 - per).abs() <= slack,
        "{}",
        second.len()
    );
    // The loop's sparkles, too.
    let sparkle_loop = (
        BENCH[SPARKLE].windows[1].0 + LOOP_MS,
        BENCH[SPARKLE].windows[1].1 + LOOP_MS,
    );
    let late: Vec<f32> = of(SPARKLE).into_iter().filter(|ms| *ms > 3000.0).collect();
    assert!(late.len() > 40, "{} looped sparkles", late.len());
    for ms in late {
        assert!(
            in_windows(&[sparkle_loop], ms, DT),
            "a looped sparkle at {ms}"
        );
    }
    // Never a second inhale.
    for layer in [INHALE, INSMOKE] {
        assert!(
            of(layer).iter().all(|ms| *ms < 1000.0),
            "{} again",
            BENCH[layer].name
        );
    }
}

// ── As drawn ─────────────────────────────────────────────────────────────────

/// Every particle wears its layer's look at its age — colour, alpha and blend
/// (the inhale and the sparkles additive and glowing, both smokes blended
/// flat), twice its keyed radius across (no camera: nothing held) — on its
/// layer's sprite (the `glow` and `smoke` falloffs read back from the texture,
/// the sparkle a four-point star) and on a unit quad.
#[test]
fn every_particle_is_drawn_as_its_layer_keys_it() {
    let (mut h, _, _) = silenced();
    let mut checked = [0usize; 4];
    let mut sample: [Option<(StandardMaterial, Handle<Mesh>)>; 4] = [None, None, None, None];
    for _ in 0..140 {
        h.tick(1);
        for (e, emitter, age, life, ..) in h.particles() {
            let layer = layer_index(emitter);
            let l = &BENCH[layer];
            let t = age / life;
            assert_drawn(
                &h.material(e),
                l.additive,
                bench_rgb(l.color, t),
                bench_keys(l.alpha, t),
                l.name,
            );
            let scale = h.app.world().get::<Transform>(e).unwrap().scale;
            let want = 2.0 * bench_keys(l.scale, t);
            assert!(
                (scale.x - want).abs() < 1e-5 && (scale.y - want).abs() < 1e-5,
                "{}: {} vs {want}",
                l.name,
                scale.x
            );
            checked[layer] += 1;
            let mesh = h.app.world().get::<Mesh3d>(e).unwrap().0.clone();
            sample[layer] = Some((h.material(e), mesh));
        }
    }
    for (layer, n) in checked.iter().enumerate() {
        assert!(*n > 30, "{}: only {n} checked", BENCH[layer].name);
    }
    for (layer, sampled) in sample.iter().enumerate() {
        let (m, mesh) = sampled.clone().unwrap();
        let w = h.width(&m);
        match BENCH[layer].falloff {
            Some(stops) => {
                for x in 0..w {
                    let (r, a) = h.texel(&m, x, w / 2);
                    let want = bench_keys(&stops, r);
                    assert!(
                        (a - want).abs() < 1.0 / 255.0 + 1e-3,
                        "{}: alpha {a} at {r}, bench {want}",
                        BENCH[layer].name
                    );
                }
            }
            None => {
                // A star: the rays (along the axes) outshine the diagonals.
                let at = (w as f32 * (0.5 + 0.35)) as u32;
                let (_, axis) = h.texel(&m, at, w / 2);
                let d = (w as f32 * (0.5 + 0.35 / 2f32.sqrt())) as u32;
                let (_, diagonal) = h.texel(&m, d, d);
                assert!(
                    axis > 0.9 && diagonal < 0.1,
                    "axis {axis}, diagonal {diagonal}"
                );
            }
        }
        let world = h.app.world();
        let mesh = world.resource::<Assets<Mesh>>().get(&mesh).unwrap();
        let corners: BTreeSet<(i32, i32, i32)> = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .unwrap()
            .iter()
            .map(|p| {
                (
                    (p[0] * 10.0).round() as i32,
                    (p[1] * 10.0).round() as i32,
                    (p[2] * 10.0).round() as i32,
                )
            })
            .collect();
        assert_eq!(
            corners,
            BTreeSet::from([(-5, -5, 0), (-5, 5, 0), (5, -5, 0), (5, 5, 0)]),
            "a unit quad"
        );
    }
}

/// The sparkles face the camera and turn as the bench turns them: each drawn
/// at its own roll — on the screen, exactly the bench's `ang`, clockwise
/// (`ctx.rotate`, y down) — that roll turning 1.1 turns a second, at 16 ms
/// and at 33 ms frames alike. Every quad faces the camera.
#[test]
fn every_quad_faces_the_camera_and_the_sparkles_turn_clockwise() {
    for dt in [DT, 0.033] {
        quads_face_the_camera_and_sparkles_turn(dt);
    }
}

fn quads_face_the_camera_and_sparkles_turn(dt: f32) {
    let (mut h, _, _) = silenced_at(dt);
    let cam = match_camera(MAGE_AT, 60.0, 38.7f32.to_radians(), 0.8);
    h.camera(cam);
    h.tick((0.32 / dt).round() as u32);
    let (right, up) = (cam.rotation * Vec3::X, cam.rotation * Vec3::Y);
    let screen_angle = |h: &Harness, e: Entity| -> f32 {
        let g = h
            .app
            .world()
            .get::<GlobalTransform>(e)
            .unwrap()
            .compute_transform();
        let x = g.rotation * Vec3::X;
        x.dot(up).atan2(x.dot(right))
    };
    let mut faced = 0;
    let before: Vec<(Entity, f32)> = h
        .particles()
        .into_iter()
        .filter(|p| layer_index(p.1) == SPARKLE && p.2 + 0.1 < p.3)
        .map(|p| (p.0, screen_angle(&h, p.0)))
        .collect();
    assert!(before.len() > 5, "{} sparkles", before.len());
    for (e, _, _, _, _, _, angle) in h.particles() {
        let g = h
            .app
            .world()
            .get::<GlobalTransform>(e)
            .unwrap()
            .compute_transform();
        assert!(
            (g.rotation * Vec3::Z).dot(cam.back().as_vec3()) > 0.9999,
            "a quad turned off the camera"
        );
        // On the screen the sprite is rolled the bench's `ang`, clockwise.
        let off = (screen_angle(&h, e) + angle + PI).rem_euclid(TAU) - PI;
        assert!(off.abs() < 1e-3, "drawn {off} rad off its roll");
        faced += 1;
    }
    assert!(faced > 10);
    h.tick(1);
    for (e, a0) in before {
        let a1 = screen_angle(&h, e);
        let turned = (a1 - a0 + PI).rem_euclid(TAU) - PI;
        let want = -BENCH[SPARKLE].spin * TAU * dt;
        assert!(
            (turned - want).abs() < 1e-3,
            "turned {turned} on the screen in a frame, bench {want}"
        );
    }
}

/// At the match camera — at its pitch and yaws all round, with the victim
/// facing toward the camera and away from it, and a low and a near camera —
/// every quad stands nearer than every point of the victim's body (the mouth
/// is inside its top sphere), on its true point's view ray and scaled to hold
/// its screen size.
#[test]
fn every_quad_stands_in_front_of_the_body_where_the_bench_puts_it() {
    let mut held = 0;
    let mut checked = 0;
    for (zoom, pitch, yaw, facing) in [
        (60.0, 38.7f32.to_radians(), 0.0, 0.0),
        (60.0, 38.7f32.to_radians(), 0.0, PI),
        (60.0, 38.7f32.to_radians(), 2.3, 1.0),
        (60.0, 38.7f32.to_radians(), 4.4, 4.0),
        (20.0, 1.5, 4.0, 2.0),
        (60.0, 0.1, 1.0, 1.0),
    ] {
        let mut h = Harness::new();
        let priest = h.priest(Vec3::new(-8.0, COMBATANT_Y, 0.0));
        let mage = h.mage(MAGE_AT, facing);
        let cam = match_camera(MAGE_AT, zoom, pitch, yaw);
        h.camera(cam);
        h.cast(priest, mage, AbilityType::Silence);
        let eye = cam.translation;
        let forward = cam.forward().as_vec3();
        let nearest = body_nearest_depth(MAGE_AT, eye, forward);
        for frames in [8, 40, 50] {
            h.tick(frames);
            let at_mouth = mouth(MAGE_AT, facing);
            for (e, emitter, age, life, offset, ..) in h.particles() {
                let g = h
                    .app
                    .world()
                    .get::<GlobalTransform>(e)
                    .unwrap()
                    .compute_transform();
                let truly = at_mouth + offset;
                let depth = (g.translation - eye).dot(forward);
                assert!(depth < nearest, "buried: {depth} behind {nearest}");
                let ray = (truly - eye).normalize();
                assert!(
                    (g.translation - eye).normalize().dot(ray) > 0.999_999,
                    "a held quad left its screen position"
                );
                let k = depth / (truly - eye).dot(forward);
                let r = bench_keys(BENCH[layer_index(emitter)].scale, age / life);
                assert!(
                    (g.scale.x - 2.0 * r * k).abs() < 1e-4,
                    "screen size {} vs {}",
                    g.scale.x,
                    2.0 * r * k
                );
                if k < 0.999 {
                    assert!(depth > nearest - 0.1, "held only as far as it must be");
                    held += 1;
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 300, "only {checked} quads checked");
    assert!(held > 150, "only {held} needed holding — vacuous");
}

// ── Lifecycle ────────────────────────────────────────────────────────────────

fn particle_count(h: &mut Harness) -> usize {
    h.particles().len()
}

/// The breath holds for the silence and no longer: it goes, particles and
/// all, the frame the silence runs out (the real `update_auras`), is
/// dispelled, or its victim dies; a refresh keeps it and its clock; it stays
/// through a stealth and hides on a sheep without losing its clock; and a
/// match that ends mid-silence takes every piece. Each case with another
/// aura on the victim that is not touched.
#[test]
fn the_breath_lives_exactly_as_long_as_the_silence_holds() {
    let other = || Aura {
        effect_type: AuraType::MovementSpeedSlow,
        duration: 30.0,
        magnitude: 0.5,
        ability_name: "Frostbolt".into(),
        ..Default::default()
    };
    let add_other = |h: &mut Harness, unit: Entity| {
        h.world()
            .get_mut::<ActiveAuras>(unit)
            .unwrap()
            .auras
            .push(other());
    };

    // Expires: the real `update_auras` runs the 5 s down.
    let (mut h, _, mage) = silenced();
    add_other(&mut h, mage);
    h.tick_secs(4.9);
    assert_eq!(h.rigs().len(), 1, "held to the silence's last frame");
    h.tick_secs(0.2);
    assert!(h.rigs().is_empty(), "gone with the silence");
    assert_eq!(particle_count(&mut h), 0);
    assert_eq!(
        h.app.world().get::<ActiveAuras>(mage).unwrap().auras.len(),
        1
    );

    // Dispelled at 1.5 s: gone that frame.
    let (mut h, _, mage) = silenced();
    add_other(&mut h, mage);
    h.tick_secs(1.5);
    assert!(particle_count(&mut h) > 0);
    h.world()
        .get_mut::<ActiveAuras>(mage)
        .unwrap()
        .auras
        .retain(|a| a.effect_type != AuraType::Silence);
    h.tick(1);
    assert!(h.rigs().is_empty(), "gone with the dispel");
    assert_eq!(particle_count(&mut h), 0);

    // Refreshed: the same breath, its clock running on.
    let (mut h, _, mage) = silenced();
    h.tick_secs(3.0);
    let (rig, _, age, _) = h.rigs()[0];
    let mut auras = h.world().get_mut::<ActiveAuras>(mage).unwrap();
    auras
        .auras
        .iter_mut()
        .find(|a| a.effect_type == AuraType::Silence)
        .unwrap()
        .duration = 5.0;
    h.tick_secs(3.0);
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1);
    assert_eq!(rigs[0].0, rig, "the same breath");
    assert!((rigs[0].2 - age - 3.0).abs() < 0.05, "its clock ran on");

    // Dies: gone, though the aura stays on the corpse.
    let (mut h, _, mage) = silenced();
    add_other(&mut h, mage);
    h.tick_secs(0.5);
    h.world().get_mut::<Combatant>(mage).unwrap().current_health = 0.0;
    h.tick(1);
    assert!(h.rigs().is_empty(), "gone with its victim");
    assert_eq!(particle_count(&mut h), 0);
    assert!(h
        .app
        .world()
        .get::<ActiveAuras>(mage)
        .unwrap()
        .auras
        .iter()
        .any(|a| a.effect_type == AuraType::Silence));

    // Stealthed: still drawn (stealth only fades the body).
    let (mut h, _, mage) = silenced();
    h.world().get_mut::<Combatant>(mage).unwrap().stealthed = true;
    h.tick_secs(0.5);
    let (rig, ..) = h.rigs()[0];
    assert_eq!(
        *h.app.world().get::<Visibility>(rig).unwrap(),
        Visibility::Inherited
    );
    assert!(particle_count(&mut h) > 0);

    // Polymorphed: hidden on the sheep, its clock running on, and back after.
    let (mut h, _, mage) = silenced();
    h.tick_secs(0.3);
    h.world().entity_mut(mage).insert(PolymorphedVisual);
    h.tick(2);
    let (rig, _, age, _) = h.rigs()[0];
    assert_eq!(
        *h.app.world().get::<Visibility>(rig).unwrap(),
        Visibility::Hidden
    );
    // Its particles inherit the rig's visibility: none overrides it.
    let particles = h.particles();
    assert!(!particles.is_empty());
    for (e, ..) in particles {
        assert_eq!(h.app.world().get::<ChildOf>(e).unwrap().parent(), rig);
        assert_eq!(
            *h.app.world().get::<Visibility>(e).unwrap(),
            Visibility::Inherited,
            "a particle shows on the sheep"
        );
    }
    h.tick_secs(1.0);
    h.world().entity_mut(mage).remove::<PolymorphedVisual>();
    h.tick(2);
    let (_, _, later, _) = h.rigs()[0];
    assert!(later > age + 1.0, "the clock ran on under the sheep");
    assert_eq!(
        *h.app.world().get::<Visibility>(rig).unwrap(),
        Visibility::Inherited
    );

    // The match ends mid-silence.
    let (mut h, _, _) = silenced();
    h.tick_secs(1.5);
    assert!(particle_count(&mut h) > 0);
    h.world().run_system_cached(cleanup_play_match).unwrap();
    assert!(h.rigs().is_empty());
    assert_eq!(particle_count(&mut h), 0);
}

// ── Every roll, the rendered body, and pets ─────────────────────────────────

/// The bench draws every particle's roll at birth, `rnd() * TAU`: across a
/// breath the birth rolls (the roll a particle is first seen with, less the
/// turn it has made since) fall in every quarter of the circle.
#[test]
fn the_birth_rolls_spread_round_the_circle() {
    let (mut h, _, _) = silenced();
    let mut seen = BTreeSet::new();
    let mut quarters = BTreeSet::new();
    let mut sparkle_quarters = BTreeSet::new();
    for _ in 0..150 {
        for (e, emitter, age, _, _, _, angle) in h.particles() {
            if !seen.insert(e) {
                continue;
            }
            let layer = layer_index(emitter);
            let born = (angle - BENCH[layer].spin * TAU * age).rem_euclid(TAU);
            let quarter = (born / (TAU / 4.0)) as u8;
            quarters.insert(quarter);
            if layer == SPARKLE {
                sparkle_quarters.insert(quarter);
            }
        }
        h.tick(1);
    }
    assert!(seen.len() > 100, "{} particles", seen.len());
    assert_eq!(quarters, BTreeSet::from([0, 1, 2, 3]));
    assert_eq!(sparkle_quarters, BTreeSet::from([0, 1, 2, 3]));
}

const PET_BODY_Y: f32 = -0.45;
const PET_STATURE: f32 = 0.55;

/// The mouth above a rendered root facing `facing`, a combatant's or a pet's.
fn mouth_above(root: Vec3, facing: Quat, pet: bool) -> Vec3 {
    let (lift, stature) = if pet {
        (
            PET_BODY_Y + (BENCH_HEAD - MOUTH_DOWN - COMBATANT_Y) * PET_STATURE,
            PET_STATURE,
        )
    } else {
        (BENCH_HEAD - MOUTH_DOWN - COMBATANT_Y, 1.0)
    };
    root + Vec3::Y * lift + facing * Vec3::Z * (MOUTH_FORWARD * stature)
}

/// The point a unit's anchors are measured from: its root, moved as far as
/// its `VisualBody` has left the body's rest pose.
fn rendered_root(unit: &Transform, local: Vec3, rest_y: f32) -> Vec3 {
    unit.translation + unit.rotation * (local - Vec3::Y * rest_y)
}

/// The nearest point's depth of the body at a rendered root: a combatant's
/// capsule, or the sphere bounding a pet's (`Capsule3d::new(0.35, 0.6)`,
/// centred 0.45 below its transform).
fn nearest_depth(root: Vec3, pet: bool, eye: Vec3, forward: Vec3) -> f32 {
    if pet {
        (root + Vec3::Y * PET_BODY_Y - eye).dot(forward) - (0.35 + 0.3)
    } else {
        body_nearest_depth(root, eye, forward)
    }
}

/// A silenced unit with a rendered body (`VisualBody` at `local`, resting at
/// `rest_y`), a pet's when `pet`.
fn silenced_with_body(pet: bool, at: Vec3, yaw: f32, local: Vec3) -> (Harness, Entity) {
    let mut h = Harness::new();
    let priest = h.priest(Vec3::new(-8.0, COMBATANT_Y, 0.0));
    let rest_y = if pet { PET_BODY_Y } else { 0.0 };
    let owner = h.mage(Vec3::new(9.0, COMBATANT_Y, 6.0), 0.0);
    let mut unit = h.world().spawn((
        Combatant::new(2, 1, CharacterClass::Mage),
        Transform::from_translation(at).with_rotation(Quat::from_rotation_y(yaw)),
    ));
    if pet {
        unit.insert(Pet {
            owner,
            pet_type: PetType::Spider,
        });
    }
    let victim = unit
        .with_child((
            VisualBody { rest_y },
            Transform::from_translation(local + Vec3::Y * rest_y),
        ))
        .id();
    h.cast(priest, victim, AbilityType::Silence);
    (h, victim)
}

/// The breath stands at the mouth of its victim's RENDERED body — the
/// `VisualBody` child, wherever the walk bob, fear bob or swing step has
/// carried it — and every particle is held in front of that body, on its true
/// point's view ray. On a combatant and on a pet (whose body rests 0.45 below
/// its transform), still, then after the victim runs four yards across the
/// view, turns, and its body bobs: the mouth and the body the particles are
/// held against are read afresh each frame.
#[test]
fn the_breath_rides_the_rendered_body_and_is_held_in_front_of_it() {
    let mut held = 0;
    let mut checked = 0;
    for pet in [false, true] {
        let at = Vec3::new(2.0, COMBATANT_Y, -3.0);
        let (mut h, victim) = silenced_with_body(pet, at, 0.5, Vec3::new(0.3, 0.25, -0.2));
        let cam = match_camera(at, 60.0, 38.7f32.to_radians(), 0.4);
        h.camera(cam);
        h.tick(20);
        let eye = cam.translation;
        let forward = cam.forward().as_vec3();
        let rest_y = if pet { PET_BODY_Y } else { 0.0 };
        for step in 0..2 {
            if step == 1 {
                let right = cam.right().as_vec3().with_y(0.0).normalize();
                let mut t = h.world().get_mut::<Transform>(victim).unwrap();
                t.translation = at + right * 4.0;
                t.rotation = Quat::from_rotation_y(2.6);
                let body = h.app.world().get::<Children>(victim).unwrap()[0];
                h.world().get_mut::<Transform>(body).unwrap().translation =
                    Vec3::new(-0.25, rest_y + 0.1, 0.35);
                h.tick(1);
            }
            let world = h.app.world();
            let unit = *world.get::<Transform>(victim).unwrap();
            let body = world.get::<Children>(victim).unwrap()[0];
            let local = world.get::<Transform>(body).unwrap().translation;
            let root = rendered_root(&unit, local, rest_y);
            let at_mouth = mouth_above(root, unit.rotation, pet);
            let rigs = h.rigs();
            assert_eq!(rigs.len(), 1);
            assert!(
                (rigs[0].3 - at_mouth).length() < 1e-4,
                "pet {pet}, step {step}: the breath at {} is not the rendered mouth {at_mouth}",
                rigs[0].3
            );
            let stature = if pet { PET_STATURE } else { 1.0 };
            let nearest = nearest_depth(root, pet, eye, forward);
            let particles = h.particles();
            assert!(particles.len() > 5, "{} particles", particles.len());
            for (e, emitter, age, life, offset, ..) in particles {
                let g = h
                    .app
                    .world()
                    .get::<GlobalTransform>(e)
                    .unwrap()
                    .compute_transform();
                let truly = at_mouth + offset;
                let depth = (g.translation - eye).dot(forward);
                assert!(
                    depth < nearest,
                    "pet {pet}, step {step}: buried, {depth} behind {nearest}"
                );
                assert!(
                    (g.translation - eye)
                        .normalize()
                        .dot((truly - eye).normalize())
                        > 0.999_999,
                    "pet {pet}, step {step}: off its true view ray"
                );
                let k = depth / (truly - eye).dot(forward);
                let r = bench_keys(BENCH[layer_index(emitter)].scale, age / life) * stature;
                assert!((g.scale.x - 2.0 * r * k).abs() < 1e-4, "screen size");
                if k < 0.999 {
                    assert!(
                        depth > nearest - 0.1,
                        "pet {pet}: held {} in front of the body, farther than it must be",
                        nearest - depth
                    );
                    held += 1;
                }
                checked += 1;
            }
        }
    }
    assert!(checked > 60, "{checked} particles checked");
    assert!(held > 20, "{held} needed holding");
}

/// A pet's breath is a pet's: born on shells and launched at speeds scaled to
/// its 0.55 stature, and drawn at 0.55 of each layer's size (no camera, so
/// nothing is held).
#[test]
fn a_pets_breath_is_scaled_to_its_body() {
    let (mut h, _) = silenced_with_body(true, Vec3::new(2.0, COMBATANT_Y, -3.0), 0.5, Vec3::ZERO);
    let mut seen = BTreeSet::new();
    let mut by_layer = [0usize; 4];
    for _ in 0..150 {
        h.tick(1);
        for (e, emitter, age, life, offset, velocity, _) in h.particles() {
            let layer = layer_index(emitter);
            let l = &BENCH[layer];
            let scale = h.app.world().get::<Transform>(e).unwrap().scale;
            let want = 2.0 * bench_keys(l.scale, age / life) * PET_STATURE;
            assert!(
                (scale.x - want).abs() < 1e-5,
                "{}: size {} vs {want}",
                l.name,
                scale.x
            );
            if !seen.insert(e) {
                continue;
            }
            by_layer[layer] += 1;
            assert!(
                (velocity.length() - l.speed * PET_STATURE).abs() < 1e-3,
                "{}: speed {}",
                l.name,
                velocity.length()
            );
            if let Launch::Inward(shell) = l.launch {
                let r = (offset - velocity * age).length() / (shell * PET_STATURE);
                assert!(
                    (0.6 - 1e-3..=1.0 + 1e-3).contains(&r),
                    "{}: shell {r}",
                    l.name
                );
            }
        }
    }
    for (layer, n) in by_layer.iter().enumerate() {
        assert!(*n > 10, "{}: {n} births", BENCH[layer].name);
    }
}
