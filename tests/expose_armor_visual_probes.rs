//! Probes for Expose Armor's visual (AS-366): Rupture's backhand and hand
//! flash on the Rogue, then the shield crest over the victim's head that
//! glints gold and cracks blue, its glow, its sparkles, its haze and its
//! shatter streaks — fired at the stroke's contact, only on a landing.
//!
//! Everything is sampled through the real graphical stack — the router
//! (`consume_instant_ability_signals`), the swing, the flash, the crest's
//! fire / animate / age / pose systems — and asserted on WORLD positions, the
//! drawn materials, meshes and textures, never on stored fields alone. One
//! probe lands Expose Armor from a Combat Rogue in a real headless match.
//!
//! The bench's numbers are transcribed here from the signed-off bench
//! (`docs/design/benches/2026-10-17-rogue-talent-bench.html`, the "Expose
//! Armor" tab), independently of the module's constants, so a wrong constant
//! fails a probe rather than moving it.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU.

use std::collections::{BTreeMap, HashSet};
use std::f32::consts::TAU;
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::time::TimeUpdateStrategy;

use arenasim::headless::runner::{build_headless_app_staged, HeadlessMatchState};
use arenasim::headless::{HeadlessMatchConfig, Stage, Unit};
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_book::AbilityBook;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::combat_core::{Avoidance, MeleeAvoided};
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraPending, Combatant, InstantAbilityFired, Pet, PetType, PlayMatchEntity,
    PolymorphedVisual, SwingStyle, VisualBody, WeaponHand, WeaponKind, WeaponSocket,
};
use arenasim::states::play_match::talents::{Build, TalentId};
use arenasim::states::play_match::{
    age_expose_armor_particles, animate_body_lean, animate_expose_armor_crests,
    animate_weapon_swings, cleanup_play_match, consume_avoided_strike_signals,
    consume_instant_ability_signals, expose_particle_launch, fire_expose_armor_crests,
    note_avoided_strikes, pose_expose_armor_crests, update_backhand_trails,
    update_finisher_hand_flashes, BackhandTrail, ExposeArmorCrest, ExposeArmorParticle,
    ExposeArmorParticlePart, ExposeArmorPending, ExposeArmorPiece, FinisherHandFlash,
    EXPOSE_ARMOR_EMITTERS,
};
use arenasim::CharacterClass;

// ── The bench, transcribed ──────────────────────────────────────────────────

/// 10 ms frames.
const TICK: Duration = Duration::from_millis(10);
const DT: f32 = 0.01;

/// `APPLY_CONTACT_MS`: the landing fires at the backhand's contact.
const CONTACT: f32 = 0.33;
/// The crest: `[vp[0], 2.25 + 1.0, vp[2]]` — a yard over the head anchor,
/// which is 2.25 above the floor; a combatant's transform is 1.0 above it.
const CREST_ABOVE_TRANSFORM: f32 = 2.25 + 1.0 - 1.0;
/// A pet's body hangs 0.45 below its transform at 0.55 of the stature.
const PET_BODY_Y: f32 = -0.45;
const PET_STATURE: f32 = 0.55;
/// `crest(p, 0.38*sc[0]*s, 0.5*sc[1]*s, ...)`.
const CREST_W: f32 = 0.38;
const CREST_H: f32 = 0.5;
const GOLD: [f32; 3] = [240.0, 212.0, 81.0];
const BLUE: [f32; 3] = [81.0, 137.0, 240.0];
/// `fade = keys([[0,0],[134,1],[2800,1],[3000,0]], t)`.
const FADE: &[(f32, f32)] = &[(0.0, 0.0), (134.0, 1.0), (2800.0, 1.0), (3000.0, 0.0)];
/// `sc = keys([[0,[1.5,1.5]],[700,[1.5,1.5]],[800,[1.5,1.7]],[900,[1.0,0.8]],[1000,[1,1]]], t)`.
const SCALE_X: &[(f32, f32)] = &[
    (0.0, 1.5),
    (700.0, 1.5),
    (800.0, 1.5),
    (900.0, 1.0),
    (1000.0, 1.0),
];
const SCALE_Y: &[(f32, f32)] = &[
    (0.0, 1.5),
    (700.0, 1.5),
    (800.0, 1.7),
    (900.0, 0.8),
    (1000.0, 1.0),
];
/// `bbGlow(p, 0.92*s, 1.0*s, keys([[0,gold],[667,gold],[1000,blue]]), keys([[0,0],[400,0.83],[2600,0.83],[3000,0]])*0.6)`.
const GLOW_W: f32 = 0.92;
const GLOW_H: f32 = 1.0;
const GLOW_GOLD: [f32; 3] = [255.0, 210.0, 0.0];
const GLOW_BLUE: [f32; 3] = [0.0, 90.0, 255.0];
const GLOW_WEIGHT: &[(f32, f32)] = &[(0.0, 0.0), (400.0, 0.83), (2600.0, 0.83), (3000.0, 0.0)];
/// The crest sprite's span over the crest's half-size (the outline's outer
/// half sits outside the shape).
const SPAN: f32 = 1.1;
/// The house's additive glow gain.
const GAIN: f32 = 2.5;

/// The emitters, as the bench's layer records carry them.
struct Layer {
    name: &'static str,
    rate: f32,
    window_ms: (f32, f32),
    /// Height of the birth box's centre above the crest (the bench's `pos`
    /// height off the head, less the crest's 1.0).
    centre: f32,
    area: f32,
    area_y: f32,
    life: f32,
    color: &'static [(f32, [f32; 3])],
    alpha: &'static [(f32, f32)],
    scale: &'static [(f32, f32)],
    spin: f32,
}

const SPARKLES: Layer = Layer {
    name: "sparkles",
    rate: 45.0,
    window_ms: (134.0, 667.0),
    // `pos: [0, 0, 1.0]` off the head: the crest's own height.
    centre: 0.0,
    area: 0.42,
    area_y: 0.6,
    life: 0.2,
    color: &[
        (0.0, [255.0, 210.0, 0.0]),
        (0.5, [255.0, 255.0, 255.0]),
        (1.0, [0.0, 246.0, 255.0]),
    ],
    alpha: &[(0.0, 1.0), (0.5, 1.0), (1.0, 0.0)],
    scale: &[(0.0, 0.02), (0.5, 0.21), (1.0, 0.02)],
    spin: 1.1,
};
const HAZE: Layer = Layer {
    name: "haze",
    rate: 25.0,
    window_ms: (500.0, 2600.0),
    centre: 0.97 - 1.0,
    area: 0.0,
    area_y: 0.0,
    life: 1.0,
    color: &[
        (0.0, [195.0, 207.0, 225.0]),
        (0.5, [186.0, 210.0, 255.0]),
        (1.0, [12.0, 0.0, 255.0]),
    ],
    alpha: &[(0.0, 0.0), (0.5, 0.2), (1.0, 0.0)],
    scale: &[(0.0, 0.24), (0.5, 0.36), (1.0, 0.57)],
    spin: 0.0,
};
const SHATTER: Layer = Layer {
    name: "shatter",
    rate: 150.0,
    window_ms: (400.0, 800.0),
    centre: 1.25 - 1.0,
    area: 0.6,
    area_y: 0.3,
    life: 0.5,
    color: &[
        (0.0, [255.0, 18.0, 115.0]),
        (0.3, [0.0, 162.0, 255.0]),
        (1.0, [120.0, 0.0, 255.0]),
    ],
    alpha: &[(0.0, 0.39), (0.3, 1.0), (1.0, 0.0)],
    scale: &[(0.0, 0.03), (0.3, 0.17), (1.0, 0.01)],
    spin: 0.0,
};
/// In the module's table order.
const LAYERS: [&Layer; 3] = [&SPARKLES, &HAZE, &SHATTER];

/// The bench's `keys`: linear between keys, held past the ends.
fn keyed(keys: &[(f32, f32)], t: f32) -> f32 {
    if t <= keys[0].0 {
        return keys[0].1;
    }
    for w in keys.windows(2) {
        if t <= w[1].0 {
            let u = (t - w[0].0) / (w[1].0 - w[0].0);
            return w[0].1 + (w[1].1 - w[0].1) * u;
        }
    }
    keys[keys.len() - 1].1
}

fn keyed_rgb(keys: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    let ch = |c: usize| {
        let k: Vec<(f32, f32)> = keys.iter().map(|(t, v)| (*t, v[c])).collect();
        keyed(&k, t)
    };
    [ch(0), ch(1), ch(2)]
}

fn crest_rgb(ms: f32) -> [f32; 3] {
    keyed_rgb(&[(0.0, GOLD), (667.0, GOLD), (1000.0, BLUE)], ms)
}

fn glow_rgb(ms: f32) -> [f32; 3] {
    keyed_rgb(
        &[(0.0, GLOW_GOLD), (667.0, GLOW_GOLD), (1000.0, GLOW_BLUE)],
        ms,
    )
}

// ── The harness ─────────────────────────────────────────────────────────────

const ROGUE_POS: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const VICTIM_POS: Vec3 = Vec3::new(0.0, 1.0, 2.0);

/// The resolution phase's retirement of avoidance markers.
fn retire_avoidances(mut commands: Commands, avoided: Query<Entity, With<MeleeAvoided>>) {
    for e in avoided.iter() {
        commands.entity(e).despawn();
    }
}

fn dagger_mount(hand: WeaponHand) -> Transform {
    let side = if hand == WeaponHand::Off { -1.0 } else { 1.0 };
    Transform::from_xyz(0.7 * side, -0.05, 0.3)
        .with_rotation(
            Quat::from_rotation_x(0.9) * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        )
        .with_scale(Vec3::splat(0.8))
}

fn add_crest_assets(app: &mut App) {
    app.add_plugins(bevy::asset::AssetPlugin::default());
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_asset::<Image>();
}

struct Harness {
    app: App,
    rogue: Entity,
    victim: Entity,
    main: Entity,
}

impl Harness {
    fn new() -> Self {
        Self::with_victim(false)
    }

    fn with_victim(pet: bool) -> Self {
        Self::with_tick(pet, TICK)
    }

    /// A harness whose frames are `tick` long.
    fn with_tick(pet: bool, tick: Duration) -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin));
        add_crest_assets(&mut app);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(tick));
        app.insert_resource(AbilityDefinitions::default());
        // The registration order of `states/mod.rs`: the router claims the
        // stroke, the swing and lean write the rig, the flash lands on it,
        // and the crest fires, emits, ages and is posed.
        app.add_systems(
            Update,
            (
                note_avoided_strikes,
                retire_avoidances,
                consume_instant_ability_signals,
                consume_avoided_strike_signals,
                animate_weapon_swings,
                animate_body_lean,
                update_finisher_hand_flashes,
                update_backhand_trails,
                fire_expose_armor_crests,
                animate_expose_armor_crests,
                age_expose_armor_particles,
                pose_expose_armor_crests,
            )
                .chain(),
        );
        let defs = AbilityDefinitions::default();
        let rogue = app
            .world_mut()
            .spawn((
                Transform::from_translation(ROGUE_POS),
                Combatant::new(1, 0, CharacterClass::Rogue).with_book(
                    AbilityBook::for_class(&defs, CharacterClass::Rogue)
                        .granting(&defs, AbilityType::ExposeArmor),
                ),
            ))
            .id();
        let body = app
            .world_mut()
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        let socket = |app: &mut App, hand: WeaponHand| {
            let rest = dagger_mount(hand);
            app.world_mut()
                .spawn((
                    WeaponSocket {
                        kind: WeaponKind::Dagger,
                        hand,
                        owner: rogue,
                        rest,
                        release_t: None,
                        aim: VICTIM_POS,
                        yaw_local: 0.0,
                        prev_owner_yaw: 0.0,
                        windup_s: 0.0,
                        swing_style: SwingStyle::Auto,
                        stroke_interval: 0.0,
                        last_s: 0.0,
                    },
                    rest,
                    Visibility::default(),
                ))
                .id()
        };
        let main = socket(&mut app, WeaponHand::Main);
        let off = socket(&mut app, WeaponHand::Off);
        app.world_mut().entity_mut(body).add_children(&[main, off]);
        app.world_mut().entity_mut(rogue).add_child(body);
        let mut victim = app.world_mut().spawn((
            Transform::from_translation(VICTIM_POS),
            Combatant::new(2, 0, CharacterClass::Warrior),
            ActiveAuras { auras: Vec::new() },
        ));
        if pet {
            victim.insert(Pet {
                owner: rogue,
                pet_type: PetType::Boar,
            });
        }
        let victim = victim.id();
        Harness {
            app,
            rogue,
            victim,
            main,
        }
    }

    /// A camera at `eye` looking at `at`, propagated before anything fires.
    fn camera_at(&mut self, eye: Vec3, at: Vec3) -> Transform {
        let cam = Transform::from_translation(eye).looking_at(at, Vec3::Y);
        self.app.world_mut().spawn((Camera3d::default(), cam));
        self.tick();
        cam
    }

    fn camera(&mut self) -> Transform {
        self.camera_at(Vec3::new(7.0, 9.0, -5.0), VICTIM_POS)
    }

    fn fire(&mut self) {
        let (rogue, victim) = (self.rogue, self.victim);
        self.app.world_mut().spawn(InstantAbilityFired {
            caster: rogue,
            target: Some(victim),
            ability: AbilityType::ExposeArmor,
            is_crit: false,
        });
    }

    /// The aura `ability`'s config builds, from `caster`, onto the victim.
    fn put(&mut self, ability: AbilityType, caster: Entity) {
        let victim = self.victim;
        let defs = AbilityDefinitions::default();
        let aura = AuraPending::from_ability(victim, caster, defs.get(&ability).unwrap())
            .expect("an aura")
            .aura;
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(victim)
            .unwrap()
            .auras
            .push(aura);
    }

    /// Land Expose Armor from `caster`, as `apply_pending_auras` would.
    fn land_from(&mut self, caster: Entity) {
        self.put(AbilityType::ExposeArmor, caster);
    }

    /// Rupture, a poison and Mortal Strike's wound on the victim too, from
    /// this Rogue and from a Warrior.
    fn crowd_the_victim(&mut self) {
        let warrior = self
            .app
            .world_mut()
            .spawn(Combatant::new(1, 1, CharacterClass::Warrior))
            .id();
        let rogue = self.rogue;
        self.put(AbilityType::Rupture, rogue);
        self.put(AbilityType::WoundPoison, rogue);
        self.put(AbilityType::MortalStrike, warrior);
    }

    /// Fire, land, and play to the frame the crest is up.
    fn landed(&mut self) {
        self.crowd_the_victim();
        let rogue = self.rogue;
        self.land_from(rogue);
        self.fire();
        for _ in 0..60 {
            self.tick();
            if !self.rigs().is_empty() {
                return;
            }
        }
        panic!("the crest never fired");
    }

    fn tick(&mut self) {
        self.app.update();
    }

    fn tick_secs(&mut self, secs: f32) {
        for _ in 0..(secs / DT).round() as u32 {
            self.tick();
        }
    }

    fn count<C: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query_filtered::<(), With<C>>();
        q.iter(self.app.world()).count()
    }

    fn entities<C: Component>(&mut self) -> Vec<Entity> {
        let mut q = self.app.world_mut().query_filtered::<Entity, With<C>>();
        q.iter(self.app.world()).collect()
    }

    /// `(rig, age)` of every crest.
    fn rigs(&mut self) -> Vec<(Entity, f32)> {
        let mut q = self.app.world_mut().query::<(Entity, &ExposeArmorCrest)>();
        q.iter(self.app.world()).map(|(e, c)| (e, c.age)).collect()
    }

    fn rig(&mut self) -> (Entity, f32) {
        let rigs = self.rigs();
        assert_eq!(rigs.len(), 1, "one crest");
        rigs[0]
    }

    fn piece(&self, which: ExposeArmorPiece) -> Entity {
        self.app
            .world()
            .iter_entities()
            .find(|e| e.get::<ExposeArmorPiece>() == Some(&which))
            .map(|e| e.id())
            .unwrap_or_else(|| panic!("no {which:?}"))
    }

    /// The one crest rig's entity.
    fn rig_entity(&self) -> Entity {
        let rigs: Vec<Entity> = self
            .app
            .world()
            .iter_entities()
            .filter(|e| e.contains::<ExposeArmorCrest>())
            .map(|e| e.id())
            .collect();
        assert_eq!(rigs.len(), 1, "one crest");
        rigs[0]
    }

    fn global(&self, e: Entity) -> Transform {
        self.app
            .world()
            .get::<GlobalTransform>(e)
            .unwrap()
            .compute_transform()
    }

    fn material(&self, e: Entity) -> StandardMaterial {
        let handle = &self
            .app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(e)
            .unwrap()
            .0;
        self.app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(handle)
            .unwrap()
            .clone()
    }

    fn image(&self, handle: &Handle<Image>) -> Image {
        self.app
            .world()
            .resource::<Assets<Image>>()
            .get(handle)
            .unwrap()
            .clone()
    }

    /// The alpha of `e`'s sprite at `(u, v)` in 0..1, row 0 at the top.
    fn texel(&self, e: Entity, u: f32, v: f32) -> f32 {
        let image = self.image(&self.material(e).base_color_texture.expect("a sprite"));
        let (w, h) = (image.width() as usize, image.height() as usize);
        let x = ((u * w as f32) as usize).min(w - 1);
        let y = ((v * h as f32) as usize).min(h - 1);
        image.data.as_ref().unwrap()[(y * w + x) * 4 + 3] as f32 / 255.0
    }

    fn vertices(&self, e: Entity) -> Vec<Vec3> {
        let handle = &self.app.world().get::<Mesh3d>(e).unwrap().0;
        let mesh = self
            .app
            .world()
            .resource::<Assets<Mesh>>()
            .get(handle)
            .unwrap();
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            VertexAttributeValues::Float32x3(v) => v.iter().map(|p| Vec3::from(*p)).collect(),
            other => panic!("unexpected positions {other:?}"),
        }
    }

    /// Every live particle: its entity, layer, true world position, velocity,
    /// age, life and start roll.
    fn particles(&mut self) -> Vec<Seen> {
        let rig_at: BTreeMap<Entity, Vec3> = {
            let mut q = self
                .app
                .world_mut()
                .query::<(Entity, &ExposeArmorCrest, &Transform)>();
            q.iter(self.app.world())
                .map(|(e, _, t)| (e, t.translation))
                .collect()
        };
        let mut q = self
            .app
            .world_mut()
            .query::<(Entity, &ExposeArmorParticle, &ChildOf)>();
        q.iter(self.app.world())
            .map(|(e, p, parent)| Seen {
                entity: e,
                layer: p.emitter,
                at: rig_at[&parent.parent()] + p.offset,
                velocity: p.velocity,
                age: p.age,
                life: p.life,
                start: p.start_angle,
            })
            .collect()
    }

    /// `(part entity, which part)` under particle `e`.
    fn parts_of(&self, e: Entity) -> Vec<(Entity, ExposeArmorParticlePart)> {
        self.app
            .world()
            .get::<Children>(e)
            .map(|c| {
                c.iter()
                    .map(|p| {
                        (
                            p,
                            *self.app.world().get::<ExposeArmorParticlePart>(p).unwrap(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The factor the hold scaled piece `e` by: its drawn depth over the
    /// crest's true depth, for the one camera.
    fn hold_of(&mut self, e: Entity) -> f32 {
        let cam = {
            let mut q = self
                .app
                .world_mut()
                .query_filtered::<&GlobalTransform, With<Camera3d>>();
            q.single(self.app.world()).unwrap().compute_transform()
        };
        let truly = self.global(self.rig_entity()).translation;
        depth(&cam, self.global(e).translation) / depth(&cam, truly)
    }

    fn kill_victim(&mut self) {
        let victim = self.victim;
        let mut c = self.app.world_mut().get_mut::<Combatant>(victim).unwrap();
        c.current_health = 0.0;
        c.is_dead = true;
    }

    fn move_victim(&mut self, to: Vec3) {
        let victim = self.victim;
        self.app
            .world_mut()
            .get_mut::<Transform>(victim)
            .unwrap()
            .translation = to;
    }

    fn victim_at(&self) -> Vec3 {
        self.app
            .world()
            .get::<Transform>(self.victim)
            .unwrap()
            .translation
    }
}

#[derive(Clone, Copy, Debug)]
struct Seen {
    entity: Entity,
    layer: usize,
    at: Vec3,
    velocity: Vec3,
    age: f32,
    life: f32,
    start: f32,
}

fn crest_over(translation: Vec3) -> Vec3 {
    translation + Vec3::Y * CREST_ABOVE_TRANSFORM
}

fn srgb(c: Color) -> [f32; 4] {
    let s = c.to_srgba();
    [s.red, s.green, s.blue, s.alpha]
}

fn assert_rgb(label: &str, got: Color, want: [f32; 3], alpha: f32) {
    let g = srgb(got);
    for (i, w) in want.iter().enumerate() {
        assert!(
            (g[i] - w / 255.0).abs() < 1e-4,
            "{label}: channel {i} is {} not {}",
            g[i] * 255.0,
            w
        );
    }
    assert!(
        (g[3] - alpha).abs() < 1e-4,
        "{label}: alpha is {} not {alpha}",
        g[3]
    );
}

/// The emissive the house's additive material carries for `rgb`: the colour
/// in linear space, times the glow gain.
fn assert_emissive(label: &str, m: &StandardMaterial, rgb: [f32; 3]) {
    let lin = Color::srgb(rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0).to_linear();
    let e = m.emissive;
    for (got, want) in [
        (e.red, lin.red * GAIN),
        (e.green, lin.green * GAIN),
        (e.blue, lin.blue * GAIN),
    ] {
        assert!((got - want).abs() < 1e-4, "{label}: emissive {e:?}");
    }
}

/// Where `p` lands in the view of a camera `cam`, as (right, up) over depth.
fn project(cam: &Transform, p: Vec3) -> Vec2 {
    let d = p - cam.translation;
    let depth = d.dot(cam.forward().as_vec3());
    Vec2::new(
        d.dot(cam.right().as_vec3()) / depth,
        d.dot(cam.up().as_vec3()) / depth,
    )
}

fn depth(cam: &Transform, p: Vec3) -> f32 {
    (p - cam.translation).dot(cam.forward().as_vec3())
}

/// The nearest depth of the victim's body: an upright capsule ±0.75 about
/// its transform, radius 0.5; a pet's, the sphere bounding its 0.35 × 0.6
/// capsule, 0.45 below its transform.
fn body_nearest(cam: &Transform, translation: Vec3, pet: bool) -> f32 {
    if pet {
        depth(cam, translation + Vec3::Y * PET_BODY_Y) - (0.35 + 0.3)
    } else {
        depth(cam, translation - Vec3::Y * 0.75).min(depth(cam, translation + Vec3::Y * 0.75)) - 0.5
    }
}

/// The match camera: 60 yd out, pitched 38.7° down, looking at `at` from the
/// bearing `yaw`.
fn match_camera(at: Vec3, yaw: f32) -> Transform {
    let pitch = 38.7_f32.to_radians();
    let back = Vec3::new(yaw.sin(), 0.0, yaw.cos()) * pitch.cos() + Vec3::Y * pitch.sin();
    Transform::from_translation(at + back * 60.0).looking_at(at, Vec3::Y)
}

// ── The stroke and the hand flash ───────────────────────────────────────────

/// Expose Armor swings Rupture's backhand, lays its trail and lights its hand
/// flash; the marker is consumed.
#[test]
fn expose_armor_swings_ruptures_backhand_with_its_hand_flash() {
    let mut h = Harness::new();
    h.camera();
    h.fire();
    h.tick();
    let s = h.app.world().get::<WeaponSocket>(h.main).unwrap();
    assert_eq!(s.swing_style, SwingStyle::Backhand);
    assert!(
        s.release_t.is_some_and(|t| t <= DT + 1e-6),
        "{:?}",
        s.release_t
    );
    assert_eq!(h.count::<FinisherHandFlash>(), 1);
    assert_eq!(h.count::<BackhandTrail>(), 1);
    assert_eq!(
        h.count::<InstantAbilityFired>(),
        0,
        "the marker is consumed"
    );
    assert_eq!(h.count::<ExposeArmorPending>(), 1, "the landing waits");
}

// ── When it fires ───────────────────────────────────────────────────────────

/// The crest goes up on the frame the backhand makes contact — never before —
/// and only on the strike that landed.
#[test]
fn the_crest_fires_at_contact_over_the_landed_victim() {
    let mut h = Harness::new();
    h.camera();
    h.crowd_the_victim();
    let rogue = h.rogue;
    h.land_from(rogue);
    h.fire();
    let mut first = None;
    for frame in 1..80 {
        h.tick();
        if first.is_none() && !h.rigs().is_empty() {
            first = Some(frame as f32 * DT);
        }
    }
    let first = first.expect("the crest fired");
    assert!(
        (first - CONTACT).abs() <= DT + 1e-4,
        "fired at {first}s, contact is {CONTACT}s"
    );
    assert_eq!(h.count::<ExposeArmorPending>(), 0);
    assert_eq!(h.rigs().len(), 1);
}

fn fires(stage: impl FnOnce(&mut Harness)) -> usize {
    let mut h = Harness::new();
    h.camera();
    stage(&mut h);
    h.fire();
    let mut seen = 0;
    for _ in 0..120 {
        h.tick();
        seen = seen.max(h.rigs().len() + h.count::<ExposeArmorParticle>());
    }
    assert_eq!(
        h.count::<ExposeArmorPending>(),
        0,
        "the held landing is dropped"
    );
    seen
}

/// A strike that landed nothing — dodged, parried, an immune victim — draws
/// nothing over the head; nor does another Rogue's Expose Armor, nor another
/// debuff of this Rogue's, nor a corpse wearing it.
#[test]
fn only_this_rogues_landed_expose_armor_raises_a_crest() {
    assert!(
        fires(|h| {
            let r = h.rogue;
            h.land_from(r);
        }) > 0
    );
    assert_eq!(fires(|_| {}), 0, "nothing landed");
    assert_eq!(
        fires(|h| {
            let other = h
                .app
                .world_mut()
                .spawn(Combatant::new(1, 1, CharacterClass::Rogue))
                .id();
            h.land_from(other);
        }),
        0,
        "another Rogue's"
    );
    assert_eq!(fires(|h| h.crowd_the_victim()), 0, "other debuffs");
    assert_eq!(
        fires(|h| {
            let r = h.rogue;
            h.land_from(r);
            h.kill_victim();
        }),
        0,
        "a corpse"
    );
}

/// A dodged or parried Expose Armor beside AS-379's avoided-strike path: the
/// Rogue AI's marker swings the one backhand and lights the one flash, the
/// sim's `MeleeAvoided` adds no second stroke, and nothing goes up over the
/// head.
#[test]
fn an_avoided_expose_armor_strikes_once_and_raises_no_crest() {
    for avoidance in [Avoidance::Dodge, Avoidance::Parry] {
        let mut h = Harness::new();
        h.camera();
        h.crowd_the_victim();
        let (rogue, victim) = (h.rogue, h.victim);
        h.fire();
        h.app.world_mut().spawn(MeleeAvoided {
            attacker: rogue,
            defender: victim,
            attack: "Expose Armor".to_string(),
            avoidance,
        });
        h.tick();
        assert_eq!(h.count::<BackhandTrail>(), 1, "{avoidance:?}");
        assert_eq!(h.count::<FinisherHandFlash>(), 1, "{avoidance:?}");
        let mut last = 0.0;
        for _ in 0..150 {
            h.tick();
            if let Some(t) = h.app.world().get::<WeaponSocket>(h.main).unwrap().release_t {
                assert!(t > last, "{avoidance:?}: the stroke restarted at {t}");
                last = t;
            }
            assert!(h.rigs().is_empty(), "{avoidance:?}: no crest");
            assert_eq!(h.count::<ExposeArmorParticle>(), 0);
        }
        assert!(last > 0.85, "{avoidance:?}: the stroke played through");
    }
}

// ── Where it stands ─────────────────────────────────────────────────────────

/// The crest, its glow and the rig stand a yard over the head — at the
/// victim's x and z — and follow it the frame it moves.
#[test]
fn the_crest_stands_a_yard_over_the_head_and_follows_the_victim() {
    let mut h = Harness::new();
    h.camera();
    h.landed();
    h.tick();
    for which in [
        ExposeArmorPiece::Glow,
        ExposeArmorPiece::CrestFill,
        ExposeArmorPiece::CrestLine,
    ] {
        let e = h.piece(which);
        let at = h.global(e).translation;
        assert!(
            at.distance(crest_over(VICTIM_POS)) < 1e-4,
            "{which:?} at {at}"
        );
    }
    let moved = Vec3::new(3.0, 1.4, -2.0);
    h.move_victim(moved);
    h.tick();
    let rig = h.rig().0;
    assert!(h.global(rig).translation.distance(crest_over(moved)) < 1e-4);
    for which in [ExposeArmorPiece::Glow, ExposeArmorPiece::CrestFill] {
        let e = h.piece(which);
        assert!(h.global(e).translation.distance(crest_over(moved)) < 1e-4);
    }
    // The particles ride with it: every one stays inside its layer's reach
    // of the crest over the moved victim.
    for p in h.particles() {
        assert!(p.at.distance(crest_over(moved)) < 1.2, "{p:?}");
    }
}

/// On a pet the crest stands over the pet's head at its stature, and is drawn
/// at its stature.
#[test]
fn a_pets_crest_stands_over_its_head_at_its_stature() {
    let mut h = Harness::with_victim(true);
    h.camera();
    h.landed();
    h.tick();
    let want = VICTIM_POS + Vec3::Y * (PET_BODY_Y + (2.25 + 1.0 - 1.0) * PET_STATURE);
    let rig = h.rig().0;
    assert!(h.global(rig).translation.distance(want) < 1e-4);
    let (_, age) = h.rig();
    let fill = h.piece(ExposeArmorPiece::CrestFill);
    let s = h.global(fill).scale;
    let k = h.hold_of(fill);
    let ms = age * 1000.0;
    assert!((s.x - CREST_W * keyed(SCALE_X, ms) * SPAN * PET_STATURE * k).abs() < 1e-4);
    assert!((s.y - CREST_H * keyed(SCALE_Y, ms) * SPAN * PET_STATURE * k).abs() < 1e-4);
}

// ── How it looks ────────────────────────────────────────────────────────────

/// Every frame of the landing, the crest's fill and line, and the glow, are
/// the bench's colour, alpha and size for the rig's age: gold to 0.667 s, blue
/// by 1.0 s; in by 0.134 s, out from 2.8 s; squashed 0.7-1.0 s; the line its
/// colour × 1.1 (capped); the fill at 0.45; the glow 0.92 × 1.0 at 0.83 × 0.6.
#[test]
fn the_crest_and_glow_are_the_benchs_for_every_frame_of_the_landing() {
    let mut h = Harness::new();
    h.camera();
    h.landed();
    let fill = h.piece(ExposeArmorPiece::CrestFill);
    let line = h.piece(ExposeArmorPiece::CrestLine);
    let glow = h.piece(ExposeArmorPiece::Glow);
    let mut sampled = HashSet::new();
    loop {
        let rigs = h.rigs();
        if rigs.is_empty() {
            break;
        }
        let ms = rigs[0].1 * 1000.0;
        if ms > 3000.0 {
            h.tick();
            continue;
        }
        let rgb = crest_rgb(ms);
        let fade = keyed(FADE, ms);
        assert_rgb(
            &format!("fill at {ms}"),
            h.material(fill).base_color,
            rgb,
            fade * 0.45,
        );
        let line_rgb = rgb.map(|c| (c * 1.1).min(255.0));
        assert_rgb(
            &format!("line at {ms}"),
            h.material(line).base_color,
            line_rgb,
            fade,
        );
        assert_emissive("line", &h.material(line), line_rgb);
        assert_emissive("fill", &h.material(fill), rgb);
        let g = glow_rgb(ms);
        assert_rgb(
            &format!("glow at {ms}"),
            h.material(glow).base_color,
            g,
            keyed(GLOW_WEIGHT, ms) * 0.6,
        );
        assert_emissive("glow", &h.material(glow), g);
        let (sx, sy) = (keyed(SCALE_X, ms), keyed(SCALE_Y, ms));
        let k = h.hold_of(fill);
        for e in [fill, line] {
            let s = h.global(e).scale;
            assert!((s.x - CREST_W * sx * SPAN * k).abs() < 1e-4, "{ms}: {s}");
            assert!((s.y - CREST_H * sy * SPAN * k).abs() < 1e-4, "{ms}: {s}");
        }
        let s = h.global(glow).scale;
        assert!(
            (s.x - GLOW_W * k).abs() < 1e-4 && (s.y - GLOW_H * k).abs() < 1e-4,
            "{s}"
        );
        // The moments the bench's keys turn.
        for (key, name) in [
            (67.0, "half in"),
            (300.0, "gold"),
            (750.0, "squash wide"),
            (833.0, "half blue"),
            (850.0, "squash flat"),
            (1500.0, "blue"),
            (2900.0, "half out"),
        ] {
            if (ms - key).abs() < 6.0 {
                sampled.insert(name);
            }
        }
        h.tick();
    }
    assert_eq!(sampled.len(), 7, "{sampled:?}");
    // Pinned values at the turns, against the bench's own numbers.
    assert_eq!(crest_rgb(667.0), GOLD);
    assert_eq!(crest_rgb(1000.0), BLUE);
}

/// Every piece of the landing composites the way the bench draws it:
/// `lighter` everywhere, so `AlphaMode::Add`, lit so the emissive shows.
#[test]
fn every_layer_is_additive() {
    let mut h = Harness::new();
    h.camera();
    h.landed();
    h.tick_secs(0.6);
    for which in [
        ExposeArmorPiece::Glow,
        ExposeArmorPiece::CrestFill,
        ExposeArmorPiece::CrestLine,
    ] {
        let m = h.material(h.piece(which));
        assert_eq!(m.alpha_mode, AlphaMode::Add, "{which:?}");
        assert!(!m.unlit, "{which:?}");
    }
    let mut layers = HashSet::new();
    for p in h.particles() {
        for (part, which) in h.parts_of(p.entity) {
            let m = h.material(part);
            assert_eq!(
                m.alpha_mode,
                AlphaMode::Add,
                "{} {which:?}",
                LAYERS[p.layer].name
            );
            assert!(!m.unlit);
            layers.insert((p.layer, which));
        }
    }
    assert_eq!(
        layers,
        HashSet::from([
            (0, ExposeArmorParticlePart::Sprite),
            (1, ExposeArmorParticlePart::Sprite),
            (2, ExposeArmorParticlePart::Sprite),
            (2, ExposeArmorParticlePart::Streak),
        ]),
        "every layer seen live at 0.6 s"
    );
}

/// The crest is a heater shield: its fill a solid shape with square shoulders
/// and a point; its line the outline and a cross, with nothing between. The
/// glow falls off as `bbGlow`'s stops. Every piece is the unit quad.
#[test]
fn the_crest_sprites_are_a_shield_and_its_outline_and_cross() {
    let mut h = Harness::new();
    h.camera();
    h.landed();
    let fill = h.piece(ExposeArmorPiece::CrestFill);
    let line = h.piece(ExposeArmorPiece::CrestLine);
    let glow = h.piece(ExposeArmorPiece::Glow);
    // A point in the crest's half-size units (x right, y down) to the sprite.
    let uv = |x: f32, y: f32| ((x / SPAN + 1.0) * 0.5, (y / SPAN + 1.0) * 0.5);
    let at = |e: Entity, x: f32, y: f32| {
        let (u, v) = uv(x, y);
        h.texel(e, u, v)
    };
    // Inside the shield, away from every line: filled, no line.
    for (x, y) in [(0.5, -0.6), (-0.5, -0.6), (0.4, 0.2), (-0.4, 0.2)] {
        assert_eq!(at(fill, x, y), 1.0, "fill at {x},{y}");
        assert_eq!(at(line, x, y), 0.0, "line at {x},{y}");
    }
    // The point at the bottom is filled; beside it, outside the curve, not.
    assert_eq!(at(fill, 0.0, 0.9), 1.0);
    for (x, y) in [(0.75, 0.75), (-0.75, 0.75), (1.08, -0.5), (0.0, -1.08)] {
        assert_eq!(at(fill, x, y), 0.0, "outside at {x},{y}");
    }
    // The outline straddles the path: both sides of the right edge and the top.
    for (x, y) in [
        (1.03, -0.5),
        (0.97, -0.5),
        (-1.03, -0.5),
        (0.3, -1.03),
        (0.3, -0.97),
    ] {
        assert_eq!(at(line, x, y), 1.0, "outline at {x},{y}");
    }
    // The cross: the upright from −0.75 to 0.7, the bar at −0.25 to ±0.6.
    for (x, y) in [
        (0.0, -0.7),
        (0.0, 0.0),
        (0.0, 0.65),
        (0.55, -0.25),
        (-0.55, -0.25),
    ] {
        assert_eq!(at(line, x, y), 1.0, "cross at {x},{y}");
    }
    for (x, y) in [
        (0.0, -0.85),
        (0.0, 0.8),
        (0.7, -0.25),
        (0.15, 0.0),
        (0.3, -0.12),
    ] {
        assert_eq!(at(line, x, y), 0.0, "off the cross at {x},{y}");
    }
    // The glow: full at the centre, 0.45 halfway out, nothing at the rim.
    assert!(at(glow, 0.0, 0.0) > 0.97);
    let half = h.texel(glow, 0.75, 0.5);
    assert!((half - 0.45).abs() < 0.03, "{half}");
    assert!(h.texel(glow, 0.995, 0.5) < 0.03);
    // The shared quad is a unit square: the scale alone sizes each piece.
    for e in [fill, line, glow] {
        let v = h.vertices(e);
        let (min, max) = v
            .iter()
            .fold((Vec3::MAX, Vec3::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
        assert_eq!((min.x, max.x, min.y, max.y), (-0.5, 0.5, -0.5, 0.5));
    }
}

// ── The particles ───────────────────────────────────────────────────────────

/// Plays one whole landing, recording every particle the first frame it is
/// seen and on every frame it lives.
fn play_out(h: &mut Harness) -> BTreeMap<Entity, Vec<(f32, Seen)>> {
    let mut seen: BTreeMap<Entity, Vec<(f32, Seen)>> = BTreeMap::new();
    loop {
        let rigs = h.rigs();
        if rigs.is_empty() {
            break;
        }
        let ms = rigs[0].1 * 1000.0;
        for p in h.particles() {
            seen.entry(p.entity).or_default().push((ms, p));
        }
        h.tick();
    }
    seen
}

/// Each layer emits the bench's count in exactly its window: 45/s for
/// 0.134-0.667 s, 25/s for 0.5-2.6 s, 150/s for 0.4-0.8 s.
#[test]
fn each_layer_emits_the_benchs_count_inside_its_window() {
    // At 10 ms frames, and at 8 ms and 33 ms: the count is the bench's
    // integral of the rate over the window whatever the frame length, so a
    // window that opens or closes mid-frame emits its share of that frame.
    for tick_ms in [10, 8, 33] {
        let tick = tick_ms as f32;
        let mut h = Harness::with_tick(false, Duration::from_millis(tick_ms));
        h.camera();
        h.landed();
        let seen = play_out(&mut h);
        for (i, layer) in LAYERS.iter().enumerate() {
            // A particle is first seen one frame old: its birth frame began
            // a frame before.
            let births: Vec<f32> = seen
                .values()
                .filter(|v| v[0].1.layer == i)
                .map(|v| v[0].0 - v[0].1.age * 1000.0)
                .collect();
            let want =
                (layer.rate * (layer.window_ms.1 - layer.window_ms.0) / 1000.0 + 1e-3).floor();
            assert_eq!(births.len() as f32, want, "{} at {tick_ms} ms", layer.name);
            let (first, last) = births
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), t| (a.min(*t), b.max(*t)));
            assert!(
                first > layer.window_ms.0 - tick - 1e-2
                    && first < layer.window_ms.0 + 1000.0 / layer.rate + tick,
                "{} at {tick_ms} ms: first born in the frame from {first} ms",
                layer.name
            );
            assert!(
                last < layer.window_ms.1 + 1e-2
                    && last > layer.window_ms.1 - 1000.0 / layer.rate - tick,
                "{} at {tick_ms} ms: last born in the frame from {last} ms",
                layer.name
            );
        }
        // The landing retires with its last haze puff: 2.6 s + its 1 s life.
        assert_eq!(h.count::<ExposeArmorParticle>(), 0, "at {tick_ms} ms");
    }
}

/// Every particle, every frame of its life, is drawn in its layer's colour,
/// alpha and size for its age — so the haze never passes 0.2, and the
/// shatter runs magenta → blue → violet.
#[test]
fn every_particle_is_the_benchs_colour_alpha_and_size_through_its_life() {
    let mut h = Harness::new();
    let cam = h.camera();
    h.landed();
    let mut checked = [0usize; 3];
    let mut haze_peak: f32 = 0.0;
    let mut stops = HashSet::new();
    for _ in 0..360 {
        if h.rigs().is_empty() {
            break;
        }
        for p in h.particles() {
            let layer = LAYERS[p.layer];
            assert_eq!(p.life, layer.life, "{}", layer.name);
            let t = (p.age / p.life).clamp(0.0, 1.0);
            let rgb = keyed_rgb(layer.color, t);
            let alpha = keyed(layer.alpha, t);
            let radius = keyed(layer.scale, t);
            for (part, which) in h.parts_of(p.entity) {
                let m = h.material(part);
                let (want_alpha, label) = match which {
                    ExposeArmorParticlePart::Sprite => (alpha, "sprite"),
                    ExposeArmorParticlePart::Streak => (alpha * 0.85, "streak"),
                };
                assert_rgb(
                    &format!("{} {label} t={t}", layer.name),
                    m.base_color,
                    rgb,
                    want_alpha,
                );
                assert_emissive(layer.name, &m, rgb);
                if which == ExposeArmorParticlePart::Sprite {
                    // The bench's size, scaled by the hold that moved it.
                    let g = h.global(part);
                    let k = depth(&cam, g.translation) / depth(&cam, p.at);
                    assert!(
                        (g.scale.x - radius * 2.0 * k).abs() < 1e-4,
                        "{}: {} vs {}",
                        layer.name,
                        g.scale.x,
                        radius * 2.0 * k
                    );
                }
            }
            if p.layer == 1 {
                haze_peak = haze_peak.max(alpha);
            }
            for (k, name) in [(0.0, "birth"), (layer.color[1].0, "mid"), (1.0, "death")] {
                if (t - k).abs() <= DT / p.life + 1e-4 {
                    stops.insert((p.layer, name));
                }
            }
            checked[p.layer] += 1;
        }
        h.tick();
    }
    assert!(checked.iter().all(|n| *n > 100), "{checked:?}");
    assert!(haze_peak <= 0.2 + 1e-6 && haze_peak > 0.19, "{haze_peak}");
    assert_eq!(
        stops.len(),
        9,
        "every layer seen at birth, mid and death: {stops:?}"
    );
    // The shatter's colour keys, at its mid of 0.3.
    assert_eq!(keyed_rgb(SHATTER.color, 0.3), [0.0, 162.0, 255.0]);
}

/// Each layer is born where the bench births it, and FILLS that space: the
/// sparkles a 0.42 × 0.6 × 0.42 box on the crest, the shatter a 0.6 × 0.3 ×
/// 0.6 box a quarter-yard over it, the haze one point just under it.
#[test]
fn births_fill_each_layers_box() {
    let reach = |i: usize, axis: usize| -> (f32, f32) {
        (0..4000u32)
            .map(|seed| expose_particle_launch(&EXPOSE_ARMOR_EMITTERS[i], 1.0, seed).0[axis])
            .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(v), b.max(v)))
    };
    for (i, layer) in LAYERS.iter().enumerate() {
        for (axis, half, centre) in [
            (0, layer.area * 0.5, 0.0),
            (1, layer.area_y * 0.5, layer.centre),
            (2, layer.area * 0.5, 0.0),
        ] {
            let (lo, hi) = reach(i, axis);
            assert!(
                lo >= centre - half - 1e-5 && hi <= centre + half + 1e-5,
                "{} axis {axis}: {lo}..{hi}",
                layer.name
            );
            if half > 0.0 {
                assert!(
                    lo < centre - half * 0.97 && hi > centre + half * 0.97,
                    "{} axis {axis} does not fill: {lo}..{hi}",
                    layer.name
                );
            }
        }
    }
    // In the running landing, every particle was born inside its box.
    let mut h = Harness::new();
    h.camera();
    h.landed();
    let seen = play_out(&mut h);
    let mut n = 0;
    for track in seen.values() {
        let (_, p) = track[0];
        let layer = LAYERS[p.layer];
        let born = p.at - p.velocity * p.age;
        let local = born - crest_over(VICTIM_POS);
        let slack = 0.02;
        assert!(
            local.x.abs() <= layer.area * 0.5 + slack,
            "{} {local}",
            layer.name
        );
        assert!(
            local.z.abs() <= layer.area * 0.5 + slack,
            "{} {local}",
            layer.name
        );
        assert!(
            (local.y - layer.centre).abs() <= layer.area_y * 0.5 + slack,
            "{} {local}",
            layer.name
        );
        n += 1;
    }
    assert!(n > 100);
}

/// The launches: sparkles and haze every way out at 0.28 and 0.56 yd/s; the
/// shatter DOWN at 0.9 yd/s within 0.5 rad of straight down, falling faster
/// under gravity 2; the haze braked by its drag.
#[test]
fn each_layer_launches_and_flies_as_the_bench_does() {
    let mut dirs = [Vec::new(), Vec::new(), Vec::new()];
    for seed in 0..3000u32 {
        for (i, dir) in dirs.iter_mut().enumerate() {
            dir.push(expose_particle_launch(&EXPOSE_ARMOR_EMITTERS[i], 1.0, seed).1);
        }
    }
    for (i, speed) in [(0, 0.28), (1, 0.56)] {
        let v = &dirs[i];
        assert!(v.iter().all(|v| (v.length() - speed).abs() < 1e-4));
        // Every way out: each octant holds a share.
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                for sz in [-1.0, 1.0] {
                    let n = v
                        .iter()
                        .filter(|v| v.x * sx > 0.0 && v.y * sy > 0.0 && v.z * sz > 0.0)
                        .count();
                    assert!(n > 200, "layer {i} octant {sx},{sy},{sz}: {n}");
                }
            }
        }
    }
    let shatter = &dirs[2];
    for v in shatter {
        assert!((v.length() - 0.9).abs() < 1e-4);
        let off_down = (-v.y / v.length()).clamp(-1.0, 1.0).acos();
        assert!(off_down <= 0.5 + 1e-4, "{off_down}");
    }
    assert!(
        shatter.iter().any(|v| (-v.y / v.length()).acos() > 0.45),
        "the cone is filled out"
    );

    // In flight: the shatter's fall speeds up by 2 yd/s each second, and
    // the haze slows by e^(−7.5) each second.
    let mut h = Harness::new();
    h.camera();
    h.landed();
    let seen = play_out(&mut h);
    let mut flown = [0, 0];
    for track in seen.values() {
        for pair in track.windows(2) {
            let ((_, a), (_, b)) = (pair[0], pair[1]);
            if (b.age - a.age - DT).abs() > 1e-4 {
                continue;
            }
            match a.layer {
                2 => {
                    assert!((b.velocity.y - (a.velocity.y - 2.0 * DT)).abs() < 1e-4);
                    assert!((b.at - a.at - b.velocity * DT).length() < 1e-4);
                    flown[0] += 1;
                }
                1 => {
                    let k = (-2.5 * 3.0 * DT).exp();
                    assert!((b.velocity - a.velocity * k).length() < 1e-5);
                    flown[1] += 1;
                }
                _ => {}
            }
        }
    }
    assert!(flown[0] > 500 && flown[1] > 500, "{flown:?}");
}

// ── Facing the camera ───────────────────────────────────────────────────────

/// Every billboard faces the camera, from wherever it stands; the round
/// pieces (crest, glow) stand upright to it, and every particle's roll is
/// exactly its own start roll turned clockwise on screen at its layer's
/// spin — 1.1 rev/s for a sparkle, none for the rest.
#[test]
fn every_piece_faces_the_camera_and_each_particle_holds_its_own_roll() {
    for eye in [
        Vec3::new(12.0, 8.0, 0.0),
        Vec3::new(-3.0, 6.0, 14.0),
        Vec3::new(0.5, 40.0, 50.0),
    ] {
        let mut h = Harness::new();
        let cam = h.camera_at(eye, VICTIM_POS);
        h.landed();
        let (cz, cx) = (cam.rotation * Vec3::Z, cam.rotation * Vec3::X);
        let mut rolled = 0;
        let mut starts = Vec::new();
        for _ in 0..120 {
            h.tick();
            for which in [
                ExposeArmorPiece::Glow,
                ExposeArmorPiece::CrestFill,
                ExposeArmorPiece::CrestLine,
            ] {
                let r = h.global(h.piece(which)).rotation;
                assert!((r * Vec3::Z).dot(cz) > 0.9999, "{which:?} from {eye}");
                assert!(
                    (r * Vec3::X).dot(cx) > 0.9999,
                    "{which:?} rolled from {eye}"
                );
            }
            for p in h.particles() {
                for (part, which) in h.parts_of(p.entity) {
                    let r = h.global(part).rotation;
                    assert!((r * Vec3::Z).dot(cz) > 0.9999, "{which:?} from {eye}");
                    if which == ExposeArmorParticlePart::Sprite {
                        let spin = LAYERS[p.layer].spin;
                        let roll = -(p.start + spin * TAU * p.age);
                        let want = cam.rotation * Quat::from_rotation_z(roll);
                        for axis in [Vec3::X, Vec3::Y] {
                            assert!(
                                (r * axis).distance(want * axis) < 1e-4,
                                "{}: {r} vs {want}",
                                LAYERS[p.layer].name
                            );
                        }
                        // Clockwise on screen: the quad's x axis, in the
                        // camera's right/up, turns the bench's way.
                        let x = r * Vec3::X;
                        let screen =
                            (x.dot(cam.up().as_vec3())).atan2(x.dot(cam.right().as_vec3()));
                        let want_screen = roll.rem_euclid(TAU);
                        let d = (screen.rem_euclid(TAU) - want_screen).abs();
                        assert!(d < 1e-3 || (TAU - d) < 1e-3, "{screen} vs {want_screen}");
                        rolled += 1;
                        starts.push((p.layer, p.start));
                    }
                }
            }
        }
        assert!(rolled > 300, "{rolled}");
        // The rolls are each particle's own: drawn from the whole turn.
        for layer in 0..3 {
            let s: Vec<f32> = starts
                .iter()
                .filter(|(l, _)| *l == layer)
                .map(|(_, s)| *s)
                .collect();
            let (lo, hi) = s
                .iter()
                .fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
            assert!(lo < 1.0 && hi > TAU - 1.0, "layer {layer}: {lo}..{hi}");
        }
    }
}

/// A streak lies in the camera's plane with its two ends on the screen where
/// the bench strokes its line: one on the particle, the other on the point
/// `min(0.48, 0.06 × speed)` back along its flight.
#[test]
fn a_streaks_ends_project_onto_the_particle_and_its_tail_point() {
    for eye in [Vec3::new(7.0, 9.0, -5.0), Vec3::new(-10.0, 4.0, 8.0)] {
        let mut h = Harness::new();
        let cam = h.camera_at(eye, VICTIM_POS);
        h.landed();
        let mut checked = 0;
        for _ in 0..120 {
            h.tick();
            for p in h.particles().into_iter().filter(|p| p.layer == 2) {
                let speed = p.velocity.length();
                let Some((part, _)) = h
                    .parts_of(p.entity)
                    .into_iter()
                    .find(|(_, w)| *w == ExposeArmorParticlePart::Streak)
                else {
                    panic!("a shatter particle has a streak");
                };
                let g = h.global(part);
                let along = g.rotation * Vec3::X * g.scale.x * 0.5;
                let (head, tail) = (g.translation - along, g.translation + along);
                let length = (4.0 * 0.12_f32).min(0.06 * speed);
                let tail_true = p.at - p.velocity / speed * length;
                let (ph, pt) = (project(&cam, head), project(&cam, tail));
                assert!(
                    ph.distance(project(&cam, p.at)) < 1e-4,
                    "head {ph} vs {}",
                    project(&cam, p.at)
                );
                assert!(
                    pt.distance(project(&cam, tail_true)) < 1e-4,
                    "tail {pt} vs {}",
                    project(&cam, tail_true)
                );
                // Its width is 0.7 of the sprite's radius.
                let radius = keyed(SHATTER.scale, (p.age / p.life).clamp(0.0, 1.0));
                let k = depth(&cam, head) / depth(&cam, p.at);
                assert!((g.scale.y - radius * 0.7 * k).abs() < 1e-4);
                // In the camera's plane: one depth across its whole face.
                assert!((g.rotation * Vec3::Z).dot(cam.rotation * Vec3::Z) > 0.9999);
                checked += 1;
            }
        }
        assert!(checked > 500, "{checked}");
    }
}

// ── In front of the body ────────────────────────────────────────────────────

/// Every drawn piece — crest, glow, every particle and streak — stands nearer
/// the camera than the victim's body does, wherever the camera is, and
/// projects where the bench puts it. At the match camera the crest and glow
/// stand clear on their own and only the lowest falling streaks are held;
/// from a camera level with the crest, everything is. A pet's body too.
#[test]
fn every_piece_stands_in_front_of_the_body_where_the_bench_draws_it() {
    for (pet, eye, crest_held) in [
        (false, match_camera(VICTIM_POS, 0.4).translation, false),
        (false, match_camera(VICTIM_POS, 2.5).translation, false),
        (false, Vec3::new(0.0, 3.25, 10.0), true),
        (false, Vec3::new(6.0, 2.6, -6.0), true),
        (true, Vec3::new(0.0, 0.8, 8.0), true),
    ] {
        let mut h = Harness::with_victim(pet);
        let cam = h.camera_at(eye, VICTIM_POS + Vec3::Y * 1.5);
        h.landed();
        let (mut held, mut crests_held) = (0, 0);
        let stature = if pet { PET_STATURE } else { 1.0 };
        let mut checked = 0;
        for _ in 0..130 {
            h.tick();
            let victim = h.victim_at();
            let nearest = body_nearest(&cam, victim, pet);
            let crest_true = h.global(h.rig_entity()).translation;
            for which in [
                ExposeArmorPiece::Glow,
                ExposeArmorPiece::CrestFill,
                ExposeArmorPiece::CrestLine,
            ] {
                let g = h.global(h.piece(which));
                let at = g.translation;
                assert!(depth(&cam, at) < nearest, "{which:?} from {eye}");
                assert!(project(&cam, at).distance(project(&cam, crest_true)) < 1e-4);
                // Held, it is scaled by the same ratio, so it covers the
                // bench's screen footprint.
                let k = depth(&cam, at) / depth(&cam, crest_true);
                let ms = h.rig().1 * 1000.0;
                let size = match which {
                    ExposeArmorPiece::Glow => Vec2::new(GLOW_W, GLOW_H),
                    _ => {
                        Vec2::new(CREST_W * keyed(SCALE_X, ms), CREST_H * keyed(SCALE_Y, ms)) * SPAN
                    }
                } * stature
                    * k;
                assert!(
                    (g.scale.truncate() - size).length() < 1e-4,
                    "{which:?} from {eye}: {} vs {size}",
                    g.scale
                );
                if at.distance(crest_true) > 1e-3 {
                    crests_held += 1;
                }
                checked += 1;
            }
            for p in h.particles() {
                for (part, which) in h.parts_of(p.entity) {
                    let g = h.global(part);
                    if which == ExposeArmorParticlePart::Sprite {
                        let k = depth(&cam, g.translation) / depth(&cam, p.at);
                        let radius = keyed(LAYERS[p.layer].scale, (p.age / p.life).clamp(0.0, 1.0));
                        assert!((g.scale.x - radius * 2.0 * stature * k).abs() < 1e-4);
                    }
                    let along = g.rotation * Vec3::X * g.scale.x * 0.5;
                    let point = match which {
                        ExposeArmorParticlePart::Sprite => g.translation,
                        ExposeArmorParticlePart::Streak => g.translation - along,
                    };
                    assert!(depth(&cam, g.translation) < nearest, "{which:?} from {eye}");
                    assert!(project(&cam, point).distance(project(&cam, p.at)) < 1e-4);
                    if point.distance(p.at) > 1e-3 {
                        held += 1;
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 300, "{checked}");
        assert_eq!(
            crests_held > 0,
            crest_held,
            "from {eye}: {crests_held} crest frames held"
        );
        assert!(held > 0, "from {eye}: some particle is held");
    }
}

/// The hold is judged against the body where it stands NOW. A victim running
/// toward the camera and round it every frame, under cameras where the hold
/// binds (level with the crest, low pitch, near zoom) and the match pitch,
/// a pet too: every frame, every drawn piece stands nearer the camera than
/// the victim's current body and projects onto its own true point.
#[test]
fn the_hold_follows_a_victim_running_under_the_camera() {
    let start = VICTIM_POS;
    let mut held = 0;
    let mut checked = 0;
    for pet in [false, true] {
        for (eye, label) in [
            (Vec3::new(0.0, 3.25, 10.0), "level with the crest"),
            (Vec3::new(-9.0, 2.0, 9.0), "low and near"),
            (Vec3::new(8.0, 0.9, 4.0), "at the floor"),
            (match_camera(start, 1.2).translation, "the match camera"),
        ] {
            let mut h = Harness::with_victim(pet);
            let cam = h.camera_at(eye, start + Vec3::Y * 1.5);
            h.landed();
            let toward = (eye - start).with_y(0.0).normalize();
            let across = Vec3::new(-toward.z, 0.0, toward.x);
            let mut t = 0.0_f32;
            while !h.rigs().is_empty() {
                t += DT;
                // Up to 3 yd toward the camera and 2 across, and back, at
                // up to about 4.5 yd/s.
                let at = start + toward * 3.0 * (t * 1.5).sin() + across * 2.0 * (t * 2.3).sin();
                h.move_victim(at);
                h.tick();
                if h.rigs().is_empty() {
                    break;
                }
                let nearest = body_nearest(&cam, at, pet);
                let crest_true = h.global(h.rig_entity()).translation;
                for which in [
                    ExposeArmorPiece::Glow,
                    ExposeArmorPiece::CrestFill,
                    ExposeArmorPiece::CrestLine,
                ] {
                    let p = h.global(h.piece(which)).translation;
                    assert!(
                        depth(&cam, p) < nearest,
                        "{which:?} behind the body {label}, pet {pet}, at {t}s"
                    );
                    assert!(project(&cam, p).distance(project(&cam, crest_true)) < 1e-4);
                    held += usize::from(p.distance(crest_true) > 1e-3);
                    checked += 1;
                }
                for p in h.particles() {
                    for (part, which) in h.parts_of(p.entity) {
                        let g = h.global(part);
                        let head = match which {
                            ExposeArmorParticlePart::Sprite => g.translation,
                            ExposeArmorParticlePart::Streak => {
                                g.translation - g.rotation * Vec3::X * g.scale.x * 0.5
                            }
                        };
                        assert!(
                            depth(&cam, g.translation) < nearest,
                            "{which:?} behind the body {label}, pet {pet}, at {t}s"
                        );
                        assert!(project(&cam, head).distance(project(&cam, p.at)) < 1e-4);
                        held += usize::from(head.distance(p.at) > 1e-3);
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 20_000 && held > 5_000, "{held} held of {checked}");
}

/// At the match camera the crest reads as a mark ON the victim: on screen it
/// stands straight over the victim, above the top of its head, at the bench's
/// size — at the landing about 12 × 16 px of a 1080p screen.
#[test]
fn at_the_match_camera_the_crest_marks_the_victim_from_above_its_head() {
    let mut h = Harness::new();
    let cam = match_camera(VICTIM_POS, 0.9);
    h.camera_at(cam.translation, VICTIM_POS);
    h.landed();
    h.tick();
    let fill = h.piece(ExposeArmorPiece::CrestFill);
    let g = h.global(fill);
    let on_screen = project(&cam, g.translation);
    let head_top = project(&cam, VICTIM_POS + Vec3::Y * 1.25);
    assert!(
        (on_screen.x - head_top.x).abs() < 1e-3,
        "{on_screen} over {head_top}"
    );
    // Its lowest point, a quarter-shield under its centre, is above the head.
    let bottom = project(&cam, g.translation - Vec3::Y * CREST_H * 1.5 * 0.5);
    assert!(bottom.y > head_top.y, "{bottom} above {head_top}");
    // Pixels a yard at that depth on a 1080-row screen with a 45° view.
    let px_per_yd = 540.0 / (22.5_f32.to_radians().tan() * depth(&cam, g.translation));
    let w_px = g.scale.x / SPAN * px_per_yd;
    let h_px = g.scale.y / SPAN * px_per_yd;
    assert!(
        (11.0..13.5).contains(&w_px) && (14.5..18.0).contains(&h_px),
        "{w_px} x {h_px}"
    );
}

// ── Lifecycle, with Rupture, a poison and Mortal Strike on the victim ────────

/// The landing plays its 3 s, its last haze puffs live out their second, and
/// the rig retires at 3.6 s with nothing left behind.
#[test]
fn the_landing_retires_with_its_last_haze_puff() {
    let mut h = Harness::new();
    h.camera();
    h.landed();
    let mut last_age = 0.0;
    while let Some((_, age)) = h.rigs().first().copied() {
        last_age = age;
        h.tick();
    }
    assert!((last_age - 3.6).abs() <= DT + 1e-4, "{last_age}");
    assert_eq!(h.count::<ExposeArmorParticle>(), 0);
    assert_eq!(h.count::<ExposeArmorParticlePart>(), 0);
    assert_eq!(h.count::<ExposeArmorPiece>(), 0);
    assert_eq!(
        h.app
            .world()
            .get::<ActiveAuras>(h.victim)
            .unwrap()
            .auras
            .len(),
        4
    );
}

/// The victim dying or despawning mid-landing ends it the next frame, with
/// every quad and particle under it.
#[test]
fn the_victim_dying_or_vanishing_mid_landing_ends_it() {
    for despawn in [false, true] {
        let mut h = Harness::new();
        h.camera();
        h.landed();
        h.tick_secs(0.6);
        assert!(h.count::<ExposeArmorParticle>() > 10);
        if despawn {
            let v = h.victim;
            h.app.world_mut().entity_mut(v).despawn();
        } else {
            h.kill_victim();
        }
        h.tick();
        assert!(h.rigs().is_empty(), "despawn {despawn}");
        assert_eq!(h.count::<ExposeArmorParticle>(), 0);
        assert_eq!(h.count::<ExposeArmorParticlePart>(), 0);
        assert_eq!(h.count::<ExposeArmorPiece>(), 0);
    }
}

/// A sheep wears nothing: polymorphed mid-landing, the crest is hidden and
/// its clock runs on; lifted, it shows again where it would have been.
#[test]
fn a_sheep_hides_the_crest_and_its_clock_runs_on() {
    let mut h = Harness::new();
    h.camera();
    h.landed();
    h.tick_secs(0.3);
    let (rig, before) = h.rig();
    let v = h.victim;
    h.app.world_mut().entity_mut(v).insert(PolymorphedVisual);
    h.tick_secs(0.5);
    assert_eq!(
        *h.app.world().get::<Visibility>(rig).unwrap(),
        Visibility::Hidden
    );
    let (_, during) = h.rig();
    assert!((during - before - 0.5).abs() < 1e-3, "the clock runs on");
    assert!(h.count::<ExposeArmorParticle>() > 0, "its particles age on");
    h.app
        .world_mut()
        .entity_mut(v)
        .remove::<PolymorphedVisual>();
    h.tick();
    assert_eq!(
        *h.app.world().get::<Visibility>(rig).unwrap(),
        Visibility::Inherited
    );
}

/// Stealth changes nothing: a victim that slips into stealth mid-landing
/// keeps its crest exactly as a twin that does not.
#[test]
fn stealth_mid_landing_changes_nothing() {
    let mut a = Harness::new();
    let mut b = Harness::new();
    for h in [&mut a, &mut b] {
        h.camera();
        h.landed();
        h.tick_secs(0.4);
    }
    let v = a.victim;
    a.app.world_mut().get_mut::<Combatant>(v).unwrap().stealthed = true;
    for _ in 0..80 {
        a.tick();
        b.tick();
        let (fa, fb) = (
            a.global(a.piece(ExposeArmorPiece::CrestLine)),
            b.global(b.piece(ExposeArmorPiece::CrestLine)),
        );
        assert_eq!(fa, fb);
        assert_eq!(
            a.material(a.piece(ExposeArmorPiece::CrestLine)).base_color,
            b.material(b.piece(ExposeArmorPiece::CrestLine)).base_color
        );
        assert_eq!(
            a.count::<ExposeArmorParticle>(),
            b.count::<ExposeArmorParticle>()
        );
    }
}

/// A second landing on the same victim inside the 3 s replaces the first: one
/// crest, its clock back at the contact, the first one's particles gone.
#[test]
fn a_recast_inside_three_seconds_replaces_the_crest() {
    let mut h = Harness::new();
    h.camera();
    h.landed();
    h.tick_secs(1.2);
    let (first, _) = h.rig();
    let old: HashSet<Entity> = h.entities::<ExposeArmorParticle>().into_iter().collect();
    assert!(!old.is_empty());
    h.fire();
    for _ in 0..40 {
        h.tick();
        if h.rigs().iter().any(|(e, _)| *e != first) {
            break;
        }
    }
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1, "one crest");
    assert_ne!(rigs[0].0, first, "the new one");
    assert!(
        rigs[0].1 <= 2.0 * DT + 1e-4,
        "its clock restarted: {}",
        rigs[0].1
    );
    assert!(h.app.world().get_entity(first).is_err());
    for e in old {
        assert!(
            h.app.world().get_entity(e).is_err(),
            "an old particle lives on"
        );
    }
    // Gold again, and fading in.
    let ms = h.rig().1 * 1000.0;
    assert_rgb(
        "fill",
        h.material(h.piece(ExposeArmorPiece::CrestFill)).base_color,
        GOLD,
        keyed(FADE, ms) * 0.45,
    );
}

/// The match ending mid-landing takes all of it: the rig and the held
/// landing are match entities, and every quad and particle hangs off the rig.
#[test]
fn the_match_ending_mid_landing_leaves_nothing_behind() {
    let mut h = Harness::new();
    h.camera();
    h.landed();
    h.tick_secs(0.6);
    let rig = h.rig().0;
    assert!(h.app.world().get::<PlayMatchEntity>(rig).is_some());
    h.fire();
    h.tick();
    for e in h.entities::<ExposeArmorPending>() {
        assert!(h.app.world().get::<PlayMatchEntity>(e).is_some());
    }
    let mut under = h.entities::<ExposeArmorParticle>();
    under.extend(h.entities::<ExposeArmorParticlePart>());
    under.extend(h.entities::<ExposeArmorPiece>());
    assert!(under.len() > 30);
    for e in &under {
        let mut at = *e;
        while let Some(parent) = h.app.world().get::<ChildOf>(at) {
            at = parent.parent();
        }
        assert_eq!(at, rig, "everything hangs off the rig");
    }
    h.app
        .world_mut()
        .run_system_once(cleanup_play_match)
        .unwrap();
    assert!(h.rigs().is_empty());
    assert_eq!(h.count::<ExposeArmorPending>(), 0);
    assert_eq!(h.count::<ExposeArmorParticle>(), 0);
    assert_eq!(h.count::<ExposeArmorParticlePart>(), 0);
    assert_eq!(h.count::<ExposeArmorPiece>(), 0);
}

// ── Through the real sim ────────────────────────────────────────────────────

/// A Combat Rogue with Expose Armor against a Warrior, played by the real
/// headless schedule with the crest's systems riding on it.
fn combat_rogue_match(stage: Stage) -> App {
    let config = HeadlessMatchConfig {
        team1: vec!["Warrior".into()],
        team2: vec!["Rogue".into()],
        random_seed: Some(5),
        max_duration_secs: 40.0,
        team2_builds: vec![Build::of(&[
            (TalentId::LightningReflexes, 2),
            (TalentId::Deflection, 2),
            (TalentId::SwiftBlades, 1),
            (TalentId::ExposeArmor, 1),
        ])
        .into()],
        ..Default::default()
    };
    let mut app = build_headless_app_staged(&config, true, None, stage);
    add_crest_assets(&mut app);
    app.add_systems(
        Update,
        (
            consume_instant_ability_signals,
            fire_expose_armor_crests,
            animate_expose_armor_crests,
            age_expose_armor_particles,
            pose_expose_armor_crests,
        )
            .chain(),
    );
    app
}

/// What a real match drew: every frame a crest stood, `(crest, victim
/// entity, victim translation, rig translation)`, and how many landings were
/// held for contact.
struct Drawn {
    crests: Vec<(Entity, Entity, Vec3, Vec3)>,
    held: usize,
    /// The frames the first landing was held, and its crest first stood.
    first_held: Option<u32>,
    first_crest: Option<u32>,
    warrior: Entity,
    rogue: Entity,
    exposed_by_rogue: bool,
}

fn play_drawn(mut app: App) -> Drawn {
    let mut crests = Vec::new();
    let mut held = HashSet::new();
    let mut exposed_by_rogue = false;
    let (mut warrior, mut rogue) = (Entity::PLACEHOLDER, Entity::PLACEHOLDER);
    let (mut first_held, mut first_crest) = (None, None);
    for frame in 0..(60 * 60) {
        app.update();
        let world = app.world_mut();
        let mut units = world.query::<(Entity, &Combatant, &Transform, Option<&ActiveAuras>)>();
        let mut at = BTreeMap::new();
        for (e, c, t, auras) in units.iter(world) {
            at.insert(e, t.translation);
            if c.class == CharacterClass::Warrior {
                warrior = e;
            } else if c.class == CharacterClass::Rogue {
                rogue = e;
            }
            if let Some(a) = auras {
                exposed_by_rogue |= a
                    .auras
                    .iter()
                    .any(|x: &Aura| x.ability_name == "Expose Armor");
            }
        }
        let mut pending = world.query::<(Entity, &ExposeArmorPending)>();
        held.extend(pending.iter(world).map(|(e, _)| e));
        if !held.is_empty() {
            first_held.get_or_insert(frame);
        }
        let mut rigs = world.query::<(Entity, &ExposeArmorCrest, &Transform)>();
        for (e, c, t) in rigs.iter(world) {
            first_crest.get_or_insert(frame);
            crests.push((e, c.target, at[&c.target], t.translation));
        }
        if world.resource::<HeadlessMatchState>().match_complete {
            break;
        }
    }
    Drawn {
        crests,
        held: held.len(),
        first_held,
        first_crest,
        warrior,
        rogue,
        exposed_by_rogue,
    }
}

/// The Combat Rogue's Expose Armor lands on the Warrior it fights, and the
/// crest stands over THAT Warrior's head — never the Rogue's — following it
/// as it moves.
#[test]
fn a_combat_rogues_expose_armor_raises_the_crest_over_the_warrior_it_exposed() {
    let drawn = play_drawn(combat_rogue_match(Stage::new()));
    assert!(drawn.exposed_by_rogue, "Expose Armor landed");
    assert!(drawn.held > 0, "the router held a landing");
    assert!(drawn.crests.len() > 100, "{} frames", drawn.crests.len());
    for (_, target, victim_at, rig_at) in &drawn.crests {
        assert_eq!(*target, drawn.warrior);
        assert_ne!(*target, drawn.rogue);
        assert!(
            rig_at.distance(crest_over(*victim_at)) < 1e-4,
            "{rig_at} over {victim_at}"
        );
    }
    // Held from the frame the Rogue struck to the backhand's contact: 0.33 s
    // of 60 Hz frames.
    let (held, crest) = (drawn.first_held.unwrap(), drawn.first_crest.unwrap());
    let waited = (crest - held) as f32 / 60.0;
    assert!(
        (waited - CONTACT).abs() <= 1.0 / 60.0 + 1e-4,
        "waited {waited}s"
    );
}

/// The same Rogue against a Warrior that dodges, or parries, every blow:
/// Expose Armor is cast and swung, and never raises a crest.
#[test]
fn a_dodged_or_parried_expose_armor_in_a_match_raises_no_crest() {
    for parry in [false, true] {
        let stage = Stage::new().combatant(Unit::new(1, 0), move |c| {
            if parry {
                c.parry_chance = 1.0;
            } else {
                c.dodge_chance = 1.0;
            }
        });
        let drawn = play_drawn(combat_rogue_match(stage));
        assert!(drawn.held > 0, "parry {parry}: Expose Armor was cast");
        assert!(!drawn.exposed_by_rogue, "parry {parry}: it never landed");
        assert!(drawn.crests.is_empty(), "parry {parry}: no crest");
    }
}
