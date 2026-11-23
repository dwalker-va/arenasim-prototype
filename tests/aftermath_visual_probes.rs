//! Probes for Aftermath's daze (`rendering/effects/aftermath.rs` and the Fire
//! arm of `slow_ring.rs`, AS-349): the client's small fire hit at the victim's
//! chest (kit 728, `fire_impactdd_chest.m2`) with the wound flinch, and the
//! shipped slow ring at the feet in the Fire school's tint.
//!
//! Three halves:
//!
//! - **The sim's marker**, driven through `apply_pending_auras` on a bare
//!   `World` with the shipped talent tree's own Aftermath proc: every landing
//!   marks the fire hit, another slow marks none, and the names route.
//! - **The fire hit**, on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin`
//!   with the chain `states/mod.rs` registers, asserting what is DRAWN under
//!   propagated `GlobalTransform`s: where each particle is and how it flies,
//!   when it is born, the colour, alpha, blend, emissive and texture falloff
//!   of the material it wears, its quad's mesh and size, and that it is drawn
//!   in front of the victim's body at the match camera.
//! - **The ring**, through the real gait and the shipped slow treatment: the
//!   Fire tint read off the material, beside other slows, a root and a DoT,
//!   as the slow expires, is dispelled and is refreshed.
//!
//! Every bench value is transcribed below as a literal, from the signed-off
//! AS-334 bench's "Aftermath daze" tab
//! (`docs/design/benches/2026-10-17-warlock-talent-bench.html`), never read
//! back from the module's own tables.

use std::collections::{BTreeMap, BTreeSet};
use std::f32::consts::{PI, TAU};
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::render::mesh::MeshAabb;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::abilities::SpellSchool;
use arenasim::states::play_match::auras::apply_pending_auras;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraApplyOwner, AuraApplyRoute, AuraLanding,
    AuraLandingKind, AuraPending, AuraType, Combatant, GameRng, HitFlinch, Pet, PetType,
    PlayMatchEntity, PolymorphedVisual, SlowBindRing, VisualBody, WalkAnim, AFTERMATH_AURA,
};
use arenasim::states::play_match::talent_procs::{effect_aura, ProcEvent, TalentProcSlot};
use arenasim::states::play_match::talents::{TalentDefinitions, TalentEffect, TalentId};
use arenasim::states::play_match::{
    age_aftermath_hit_particles, animate_aftermath_hits, cleanup_play_match, place_aftermath_hits,
    spawn_aftermath_hits, update_slow_rings, update_slow_scuffs, update_slow_treatment,
    update_walk_animation, AbilityType, AftermathHitParticle, AftermathHitRig, SlowTint,
};
use arenasim::CharacterClass;

// ── The bench, transcribed ───────────────────────────────────────────────────

/// `ANCH.chest`, above the floor; combatants stand with their capsule centre
/// 1.0 above it.
const CHEST_ABOVE_FLOOR: f32 = 1.45;
const CHEST_ABOVE_CENTRE: f32 = CHEST_ABOVE_FLOOR - 1.0;

/// Layer indices, in the module's table order.
const FLARES: usize = 0;
const GLOW: usize = 1;

struct Layer {
    speed: f32,
    gravity: f32,
    life: f32,
    rate: f32,
    color: [[u8; 3]; 3],
    /// Alpha at life 0 / 0.5 / 1.
    alpha: [f32; 3],
    /// Sprite radius at life 0 / 0.5 / 1.
    scale: [f32; 3],
}

const BENCH: [Layer; 2] = [
    // `hit`: kind 'star', vRange 3.14, speed 1.67, gravity 1.39, life 1.0,
    // rate 60, alpha [[0,1],[1,0]].
    Layer {
        speed: 1.67,
        gravity: 1.39,
        life: 1.0,
        rate: 60.0,
        color: [[214, 0, 0], [250, 149, 18], [214, 0, 0]],
        alpha: [1.0, 0.5, 0.0],
        scale: [0.22, 0.08, 0.03],
    },
    // `hitsmoke`: kind 'cloud', vRange 3.14, speed 0.5, life 0.9, rate 50.
    Layer {
        speed: 0.5,
        gravity: 0.0,
        life: 0.9,
        rate: 50.0,
        color: [[249, 234, 64], [248, 128, 30], [255, 25, 25]],
        alpha: [0.0, 0.6, 0.0],
        scale: [0.03, 0.42, 0.14],
    },
];
/// Both layers' `win: [[0,0],[67,1],[700,0]]`, seconds.
const WINDOW: (f32, f32) = (0.067, 0.700);
/// The house's 2.5x emissive glow over an additive particle's colour.
const GLOW_GAIN: f32 = 2.5;
/// The Fire school's colour, `SpellSchool::Fire` (`abilities.rs`).
const FIRE_RGB: [u8; 3] = [255, 128, 64];

/// A three-key track (keys at 0, 0.5, 1), linear between.
fn track(keys: [f32; 3], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.5 {
        keys[0] + (keys[1] - keys[0]) * (t / 0.5)
    } else {
        keys[1] + (keys[2] - keys[1]) * ((t - 0.5) / 0.5)
    }
}

/// The bench's star sprite, at radius `r` (rim 1) and bearing `angle`: two
/// crossed ellipses of half-thickness 0.12 and a core fading out by 0.45. The
/// white of the core is tinted here (the module's departure 2), so the two
/// add up to at most full.
fn bench_star(r: f32, angle: f32) -> f32 {
    let (x, y) = (r * angle.cos(), r * angle.sin());
    let ellipse = |a: f32, b: f32| a * a + (b / 0.12).powi(2) <= 1.0;
    let ray = if ellipse(x, y) || ellipse(y, x) {
        1.0
    } else {
        0.0
    };
    (ray + (1.0 - r / 0.45).clamp(0.0, 1.0)).min(1.0)
}

/// The bench's `cloud` sprite: 0.9 at the centre, 0.6 at 0.45, 0 at the rim.
fn bench_cloud(r: f32) -> f32 {
    if r <= 0.45 {
        0.9 + (0.6 - 0.9) * (r / 0.45)
    } else {
        (0.6 * (1.0 - (r - 0.45) / 0.55)).max(0.0)
    }
}

// ── The body and the camera ──────────────────────────────────────────────────

/// Combatants stand with capsule centres at world y = 1.0.
const COMBATANT_Y: f32 = 1.0;
/// Pets stand at sim y = 0.75; the rendered body hangs 0.45 below that and is
/// 0.55 of a combatant (`IMPACT_PET_BODY_Y`, `IMPACT_PET_STATURE`).
const PET_SIM_Y: f32 = 0.75;
const PET_BODY_Y: f32 = -0.45;
const PET_STATURE: f32 = 0.55;
/// The combatant body mesh, `Capsule3d::new(0.5, 1.5)`.
const BODY_RADIUS: f32 = 0.5;
const BODY_HALF_SEGMENT: f32 = 0.75;
/// The wound flinch every landed melee auto plays (`hit_reaction.rs`): a
/// 0.10 yd dip over 0.35 s, 0.67 of that on a pet.
const FLINCH_DIP: f32 = 0.10;
const FLINCH_SECS: f32 = 0.35;
const PET_FLINCH_SCALE: f32 = 0.67;

/// The match camera at `zoom` and `pitch` from the vertical, and `yaw`,
/// looking at `target`.
fn match_camera(target: Vec3, zoom: f32, pitch: f32, yaw: f32) -> Transform {
    let at = target
        + Vec3::new(
            zoom * pitch.sin() * yaw.sin(),
            zoom * pitch.cos(),
            zoom * pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

// ── the sim's marker ─────────────────────────────────────────────────────────

fn world() -> World {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(GameRng::from_seed(7));
    world.insert_resource(Time::<()>::default());
    world
}

fn landings(world: &mut World) -> Vec<(Entity, AuraLandingKind)> {
    let mut q = world.query::<&AuraLanding>();
    q.iter(world).map(|l| (l.target, l.kind)).collect()
}

/// Aftermath's slow as its proc lands it, through the engine's own
/// constructor, from the shipped talent tree, on `victim`.
fn shipped_aftermath(victim: Entity) -> AuraPending {
    let defs = TalentDefinitions::shipped();
    let node = defs
        .iter()
        .find(|(id, _)| *id == TalentId::Aftermath)
        .map(|(_, node)| node)
        .expect("Aftermath is a talent");
    let proc = node
        .per_rank
        .iter()
        .find_map(|e| match e {
            TalentEffect::Proc(p) => Some(p.clone()),
            _ => None,
        })
        .expect("Aftermath carries a proc");
    let slot = TalentProcSlot::new(TalentId::Aftermath, &node.name, proc);
    let owner = Entity::from_raw(1);
    let event = ProcEvent::new(owner, slot.proc.trigger)
        .ability(AbilityType::Shadowbolt)
        .amount(50.0)
        .other(victim);
    effect_aura(&slot, owner, &event).expect("Aftermath lands an aura")
}

fn slow(name: &str, magnitude: f32) -> Aura {
    Aura {
        effect_type: AuraType::MovementSpeedSlow,
        duration: 5.0,
        magnitude,
        break_on_damage_threshold: -1.0,
        ability_name: name.to_string(),
        ..Default::default()
    }
}

/// The router's name is the shipped tree's: a rename in `talents.ron` must
/// not silently drop the hit or the tint.
#[test]
fn the_routed_name_is_the_talent_trees() {
    let landed = shipped_aftermath(Entity::from_raw(2));
    assert_eq!(landed.aura.ability_name, AFTERMATH_AURA);
    assert_eq!(landed.aura.effect_type, AuraType::MovementSpeedSlow);
    assert_eq!(
        AuraLandingKind::for_aura(AuraType::MovementSpeedSlow, AFTERMATH_AURA),
        Some(AuraLandingKind::AftermathFireHit)
    );
    assert_eq!(SlowTint::for_slow(AFTERMATH_AURA), Some(SlowTint::Fire));
    // Only the slow plays it: an aura of another type by the same name, and
    // another slow, land nothing.
    assert_eq!(
        AuraLandingKind::for_aura(AuraType::DamageOverTime, AFTERMATH_AURA),
        None
    );
    assert_eq!(
        AuraLandingKind::for_aura(AuraType::MovementSpeedSlow, "Frostbolt"),
        None
    );
}

/// The fire hit draws the slow's application, so the band stays off it, as
/// it does every slow; another slow keeps the slow ring's route.
#[test]
fn the_fire_hit_owns_the_application() {
    let route = AuraApplyRoute::for_instance;
    assert_eq!(
        route(AuraType::MovementSpeedSlow, AFTERMATH_AURA),
        AuraApplyRoute::Owned(AuraApplyOwner::AuraLanding)
    );
    assert_eq!(
        route(AuraType::MovementSpeedSlow, "Frostbolt"),
        AuraApplyRoute::Owned(AuraApplyOwner::SlowRing)
    );
}

#[test]
fn every_aftermath_landing_marks_its_hit_and_another_slow_none() {
    let mut world = world();
    let victim = world
        .spawn((
            Combatant::new(2, 0, CharacterClass::Warrior),
            Transform::default(),
        ))
        .id();
    world.spawn(AuraPending {
        target: victim,
        aura: slow("Frostbolt", 0.6),
    });
    world.run_system_once(apply_pending_auras).unwrap();
    assert!(
        landings(&mut world).is_empty(),
        "a Frostbolt slow marks none"
    );

    world.spawn(shipped_aftermath(victim));
    world.run_system_once(apply_pending_auras).unwrap();
    assert_eq!(
        landings(&mut world),
        vec![(victim, AuraLandingKind::AftermathFireHit)]
    );
    // A second proc while the slow is up plays a second hit.
    world.spawn(shipped_aftermath(victim));
    world.run_system_once(apply_pending_auras).unwrap();
    assert_eq!(
        landings(&mut world),
        vec![
            (victim, AuraLandingKind::AftermathFireHit),
            (victim, AuraLandingKind::AftermathFireHit)
        ]
    );
}

// ── the fire hit: harness ────────────────────────────────────────────────────

const TICK: Duration = Duration::from_millis(16);
const TICK_SECS: f32 = 0.016;

/// One particle as it was born: its layer, its rig's age then, its launch
/// velocity.
#[derive(Clone, Copy, Debug)]
struct Birth {
    emitter: usize,
    at_rig_age: f32,
    velocity: Vec3,
}

#[derive(Resource, Default)]
struct Births(BTreeMap<Entity, Birth>);

fn record_births(
    mut births: ResMut<Births>,
    born: Query<(Entity, &AftermathHitParticle, &ChildOf), Added<AftermathHitParticle>>,
    rigs: Query<&AftermathHitRig>,
) {
    for (e, p, parent) in born.iter() {
        let rig = rigs.get(parent.parent()).expect("a particle has its rig");
        births.0.insert(
            e,
            Birth {
                emitter: p.emitter,
                at_rig_age: rig.age,
                velocity: p.velocity,
            },
        );
    }
}

/// One live particle, read back from the world.
#[derive(Clone, Copy, Debug)]
struct Seen {
    entity: Entity,
    emitter: usize,
    /// Where it is DRAWN.
    drawn: Vec3,
    /// Where it truly is: its rig's chest plus its offset.
    truly: Vec3,
    velocity: Vec3,
    age: f32,
    scale: Vec3,
}

struct Harness {
    app: App,
    /// Seconds a frame lasts.
    frame: f32,
}

impl Harness {
    fn new() -> Self {
        Self::ticking(TICK)
    }

    /// A harness whose frames are `tick` long.
    fn ticking(tick: Duration) -> Self {
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
        app.init_resource::<Births>();
        app.add_systems(
            Update,
            (
                spawn_aftermath_hits,
                animate_aftermath_hits,
                record_births,
                age_aftermath_hit_particles,
                place_aftermath_hits,
            )
                .chain(),
        );
        // Warm the clock: Bevy's first update has a zero delta.
        app.update();
        Harness {
            app,
            frame: tick.as_secs_f32(),
        }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn tick_secs(&mut self, secs: f32) {
        self.tick((secs / self.frame).round() as u32);
    }

    fn victim(&mut self, at: Vec3, auras: Vec<Aura>) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Combatant::new(2, 0, CharacterClass::Warrior),
                Transform::from_translation(at),
                ActiveAuras { auras },
            ))
            .id()
    }

    fn pet_victim(&mut self, at: Vec3) -> Entity {
        let owner = self
            .app
            .world_mut()
            .spawn(Combatant::new(2, 0, CharacterClass::Hunter))
            .id();
        self.app
            .world_mut()
            .spawn((
                Combatant::new(2, 1, CharacterClass::Hunter),
                Transform::from_translation(at),
                Pet {
                    owner,
                    pet_type: PetType::Boar,
                },
            ))
            .id()
    }

    fn land(&mut self, target: Entity, kind: AuraLandingKind) {
        self.app.world_mut().spawn((
            AuraLanding {
                target,
                kind,
                caster: None,
            },
            PlayMatchEntity,
        ));
    }

    fn camera(&mut self, transform: Transform) {
        self.app.world_mut().spawn((
            Camera3d::default(),
            transform,
            GlobalTransform::from(transform),
        ));
    }

    fn count<C: Component>(&mut self) -> usize {
        let world = self.app.world_mut();
        world.query::<&C>().iter(world).count()
    }

    fn markers(&mut self) -> Vec<AuraLandingKind> {
        let world = self.app.world_mut();
        world
            .query::<&AuraLanding>()
            .iter(world)
            .map(|l| l.kind)
            .collect()
    }

    fn rigs(&mut self) -> Vec<(Entity, Entity, Vec3, Visibility, bool)> {
        let world = self.app.world_mut();
        world
            .query::<(
                Entity,
                &AftermathHitRig,
                &GlobalTransform,
                &Visibility,
                Has<PlayMatchEntity>,
            )>()
            .iter(world)
            .map(|(e, r, g, v, tagged)| (e, r.target, g.translation(), *v, tagged))
            .collect()
    }

    fn particles(&mut self) -> Vec<Seen> {
        let world = self.app.world_mut();
        let chests: Vec<(Entity, Vec3)> = world
            .query::<(Entity, &AftermathHitRig, &GlobalTransform)>()
            .iter(world)
            .map(|(e, _, g)| (e, g.translation()))
            .collect();
        world
            .query::<(Entity, &AftermathHitParticle, &GlobalTransform, &ChildOf)>()
            .iter(world)
            .map(|(entity, p, g, parent)| {
                let chest = chests
                    .iter()
                    .find(|(rig, _)| *rig == parent.parent())
                    .expect("a particle's rig is live")
                    .1;
                let (scale, _, drawn) = g.to_scale_rotation_translation();
                Seen {
                    entity,
                    emitter: p.emitter,
                    drawn,
                    truly: chest + p.offset,
                    velocity: p.velocity,
                    age: p.age,
                    scale,
                }
            })
            .collect()
    }

    fn material_of(&mut self, particle: Entity) -> StandardMaterial {
        let world = self.app.world();
        let handle = world
            .get::<MeshMaterial3d<StandardMaterial>>(particle)
            .unwrap()
            .0
            .clone();
        world
            .resource::<Assets<StandardMaterial>>()
            .get(&handle)
            .unwrap()
            .clone()
    }

    fn mesh_of(&mut self, particle: Entity) -> Mesh {
        let world = self.app.world();
        let handle = world.get::<Mesh3d>(particle).unwrap().0.clone();
        world
            .resource::<Assets<Mesh>>()
            .get(&handle)
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

    fn births(&self) -> Vec<Birth> {
        self.app
            .world()
            .resource::<Births>()
            .0
            .values()
            .copied()
            .collect()
    }

    fn set_health(&mut self, e: Entity, health: f32) {
        self.app
            .world_mut()
            .get_mut::<Combatant>(e)
            .unwrap()
            .current_health = health;
    }

    fn move_to(&mut self, e: Entity, at: Vec3) {
        self.app
            .world_mut()
            .get_mut::<Transform>(e)
            .unwrap()
            .translation = at;
    }
}

fn chest(at: Vec3) -> Vec3 {
    at + Vec3::Y * CHEST_ABOVE_CENTRE
}

/// An sRGB channel as the 8-bit value the bench wrote.
fn byte(c: f32) -> u8 {
    (c * 255.0).round() as u8
}

fn rgb(color: Color) -> [u8; 3] {
    let c = color.to_srgba();
    [byte(c.red), byte(c.green), byte(c.blue)]
}

/// One hit, landed on a fresh victim, played for `secs`.
fn played(secs: f32) -> (Harness, Entity, Vec3) {
    let mut h = Harness::new();
    let at = Vec3::new(3.0, COMBATANT_Y, -2.0);
    let victim = h.victim(at, vec![slow(AFTERMATH_AURA, 0.7)]);
    h.land(victim, AuraLandingKind::AftermathFireHit);
    h.tick_secs(secs);
    (h, victim, at)
}

// ── the landing ──────────────────────────────────────────────────────────────

#[test]
fn the_hit_stands_at_the_victims_chest_and_is_a_play_match_entity() {
    let (mut h, victim, at) = played(0.2);
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1);
    let (_, target, translation, _, tagged) = rigs[0];
    assert_eq!(target, victim);
    assert!(
        translation.distance(chest(at)) < 1e-5,
        "{translation:?} is not the chest {:?}",
        chest(at)
    );
    assert!(tagged, "the rig is a PlayMatchEntity");
    assert!(h.markers().is_empty(), "the landing marker is consumed");
}

#[test]
fn a_pets_hit_stands_at_its_chest() {
    let mut h = Harness::new();
    let at = Vec3::new(-1.0, PET_SIM_Y, 4.0);
    let pet = h.pet_victim(at);
    h.land(pet, AuraLandingKind::AftermathFireHit);
    h.tick(2);
    let rigs = h.rigs();
    assert_eq!(rigs.len(), 1);
    let expected = at + Vec3::Y * (PET_BODY_Y + CHEST_ABOVE_CENTRE * PET_STATURE);
    assert!(rigs[0].2.distance(expected) < 1e-5, "{:?}", rigs[0].2);
}

/// On a pet the whole burst is drawn at the pet's stature: launched slower,
/// falling softer and drawn smaller, in proportion.
#[test]
fn a_pets_burst_is_scaled_to_the_pet() {
    let mut h = Harness::new();
    let pet = h.pet_victim(Vec3::new(-1.0, PET_SIM_Y, 4.0));
    h.land(pet, AuraLandingKind::AftermathFireHit);
    h.tick_secs(0.4);
    let births = h.births();
    assert!(births.len() > 20);
    for b in &births {
        let speed = BENCH[b.emitter].speed * PET_STATURE;
        assert!(
            (b.velocity.length() - speed).abs() < 1e-4,
            "layer {} launches at {} on a pet",
            b.emitter,
            b.velocity.length()
        );
    }
    let before: BTreeMap<Entity, Seen> = h.particles().into_iter().map(|p| (p.entity, p)).collect();
    h.tick(1);
    let mut n = 0;
    for p in h.particles() {
        let layer = &BENCH[p.emitter];
        let radius = track(layer.scale, p.age / layer.life) * PET_STATURE;
        assert!(
            (p.scale.x - 2.0 * radius).abs() < 1e-4,
            "layer {}",
            p.emitter
        );
        if let Some(b) = before.get(&p.entity) {
            let dv = p.velocity.y - b.velocity.y;
            assert!((dv + layer.gravity * PET_STATURE * TICK_SECS).abs() < 1e-5);
            n += 1;
        }
    }
    assert!(n > 10);
}

#[test]
fn the_hit_flinches_its_victim_and_a_pet_for_its_shorter_wound() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), vec![]);
    let pet = h.pet_victim(Vec3::new(5.0, PET_SIM_Y, 0.0));
    h.land(victim, AuraLandingKind::AftermathFireHit);
    h.land(pet, AuraLandingKind::AftermathFireHit);
    h.tick(1);
    let flinch = |h: &Harness, e| {
        let f = h.app.world().get::<HitFlinch>(e).expect("the hit flinches");
        (f.duration, f.depth)
    };
    let (secs, depth) = flinch(&h, victim);
    assert!((secs - FLINCH_SECS).abs() < 1e-6 && (depth - FLINCH_DIP).abs() < 1e-6);
    let (secs, depth) = flinch(&h, pet);
    assert!((secs - FLINCH_SECS * PET_FLINCH_SCALE).abs() < 1e-6);
    assert!((depth - FLINCH_DIP).abs() < 1e-6);
}

/// Another renderer's landing is left alone: no hit, no flinch, its marker
/// kept.
#[test]
fn another_landing_plays_no_hit() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), vec![]);
    h.land(victim, AuraLandingKind::ShadowTrance);
    h.tick(5);
    assert!(h.rigs().is_empty());
    assert!(h.app.world().get::<HitFlinch>(victim).is_none());
    assert_eq!(h.markers(), vec![AuraLandingKind::ShadowTrance]);
}

#[test]
fn a_corpse_gets_no_hit_and_no_flinch() {
    let mut h = Harness::new();
    let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), vec![]);
    h.set_health(victim, 0.0);
    h.land(victim, AuraLandingKind::AftermathFireHit);
    h.tick(10);
    assert!(h.rigs().is_empty());
    assert_eq!(h.count::<AftermathHitParticle>(), 0);
    assert!(h.app.world().get::<HitFlinch>(victim).is_none());
    assert!(h.markers().is_empty(), "the marker is still consumed");
}

// ── emission ─────────────────────────────────────────────────────────────────

/// Each layer emits only inside its window, fills it from end to end, and
/// lays down the bench's count over it — at 60 frames a second, and at
/// twice and half that, so the count is the rate's and not the frame rate's.
#[test]
fn each_layer_emits_its_count_across_its_window() {
    for ms in [16, 8, 33] {
        assert_bench_emission(Duration::from_millis(ms));
    }
}

fn assert_bench_emission(tick: Duration) {
    let frame = tick.as_secs_f32();
    let mut h = Harness::ticking(tick);
    let victim = h.victim(Vec3::new(3.0, COMBATANT_Y, -2.0), vec![]);
    h.land(victim, AuraLandingKind::AftermathFireHit);
    h.tick_secs(1.8);
    let births = h.births();
    for (i, layer) in BENCH.iter().enumerate() {
        let ages: Vec<f32> = births
            .iter()
            .filter(|b| b.emitter == i)
            .map(|b| b.at_rig_age)
            .collect();
        let expected = layer.rate * (WINDOW.1 - WINDOW.0);
        assert!(
            (ages.len() as f32 - expected).abs() <= 1.0,
            "{frame} s frames, layer {i}: {} births, the bench lays {expected}",
            ages.len()
        );
        let first = ages.iter().copied().fold(f32::MAX, f32::min);
        let last = ages.iter().copied().fold(0.0, f32::max);
        // Births stand inside the window, and fill it: the first within a
        // gap of the opening, the last within a gap of the close.
        let gap = 1.0 / layer.rate + frame;
        assert!(
            first >= WINDOW.0 - 1e-4 && first < WINDOW.0 + gap,
            "{frame} s frames, layer {i}: first birth at {first}"
        );
        assert!(
            last < WINDOW.1 && last > WINDOW.1 - gap,
            "{frame} s frames, layer {i}: last birth at {last}"
        );
    }
}

/// Born at the chest, flying straight out in every direction at the bench's
/// speed.
#[test]
fn both_layers_burst_out_of_the_chest_in_every_direction() {
    let (h, _, _) = played(1.0);
    let births = h.births();
    for (i, layer) in BENCH.iter().enumerate() {
        let dirs: Vec<Vec3> = births
            .iter()
            .filter(|b| b.emitter == i)
            .map(|b| {
                assert!(
                    (b.velocity.length() - layer.speed).abs() < 1e-4,
                    "layer {i} launches at {}",
                    b.velocity.length()
                );
                b.velocity.normalize()
            })
            .collect();
        assert!(dirs.len() > 25, "layer {i}: {} births", dirs.len());
        let octants: BTreeSet<(bool, bool, bool)> = dirs
            .iter()
            .map(|d| (d.x > 0.0, d.y > 0.0, d.z > 0.0))
            .collect();
        assert_eq!(octants.len(), 8, "layer {i} fills the sphere");
        let mean = dirs.iter().copied().sum::<Vec3>() / dirs.len() as f32;
        assert!(mean.length() < 0.3, "layer {i} favours a side: {mean:?}");
    }
}

/// The flares fall under the bench's gravity and the glow does not; each
/// flies the line its velocity integrates, from the chest.
#[test]
fn the_flares_fall_and_the_glow_drifts_straight() {
    let (mut h, _, at) = played(0.3);
    let before: BTreeMap<Entity, Seen> = h.particles().into_iter().map(|p| (p.entity, p)).collect();
    h.tick(1);
    let after = h.particles();
    let mut seen = [0usize; 2];
    for p in &after {
        let Some(b) = before.get(&p.entity) else {
            continue;
        };
        seen[p.emitter] += 1;
        let dv = p.velocity - b.velocity;
        let g = BENCH[p.emitter].gravity;
        assert!(
            (dv.y + g * TICK_SECS).abs() < 1e-5 && dv.x.abs() < 1e-6 && dv.z.abs() < 1e-6,
            "layer {} changed velocity by {dv:?}",
            p.emitter
        );
        // Velocity first, then position (the bench's order).
        let step = p.truly - b.truly;
        assert!(
            (step - p.velocity * TICK_SECS).length() < 1e-5,
            "layer {} stepped {step:?}",
            p.emitter
        );
    }
    assert!(seen[FLARES] > 5 && seen[GLOW] > 5, "{seen:?}");
    // Every particle's true path starts at the chest: launch velocity less
    // what gravity took, integrated.
    for p in h.particles() {
        let n = (p.age / TICK_SECS).round();
        let g = BENCH[p.emitter].gravity;
        let v0 = p.velocity + Vec3::Y * g * TICK_SECS * n;
        // Semi-implicit Euler: x = v0 * n dt - g dt^2 n(n+1)/2.
        let expected = chest(at) + v0 * TICK_SECS * n
            - Vec3::Y * g * TICK_SECS * TICK_SECS * n * (n + 1.0) / 2.0;
        assert!(
            p.truly.distance(expected) < 1e-4,
            "layer {} is at {:?}, its flight says {expected:?}",
            p.emitter,
            p.truly
        );
    }
}

#[test]
fn the_burst_follows_its_victim() {
    let (mut h, victim, at) = played(0.3);
    let before: BTreeMap<Entity, Seen> = h.particles().into_iter().map(|p| (p.entity, p)).collect();
    let moved = at + Vec3::new(2.0, 0.0, -1.5);
    h.move_to(victim, moved);
    h.tick(1);
    assert!(h.rigs()[0].2.distance(chest(moved)) < 1e-5);
    let mut n = 0;
    for p in h.particles() {
        if let Some(b) = before.get(&p.entity) {
            n += 1;
            let carried = (p.truly - b.truly) - p.velocity * TICK_SECS;
            assert!(
                carried.distance(moved - at) < 1e-4,
                "a particle was carried {carried:?}"
            );
        }
    }
    assert!(n > 10, "only {n} particles followed");
}

/// The burst is gone by 1.7 s — the last window close plus a flare's life —
/// and not before its last flare.
#[test]
fn the_hit_retires_after_its_last_particle() {
    let (mut h, _, _) = played(1.6);
    assert_eq!(h.rigs().len(), 1, "still playing at 1.6 s");
    assert!(
        h.particles().iter().any(|p| p.emitter == FLARES),
        "the last flares are still falling at 1.6 s"
    );
    h.tick_secs(0.15);
    assert!(h.rigs().is_empty(), "retired by 1.75 s");
    assert_eq!(h.count::<AftermathHitParticle>(), 0);
}

// ── what is drawn ────────────────────────────────────────────────────────────

/// Both layers are additive (the bench composites both `lighter`), lit
/// emissive at the house glow, and step along the bench's colour and alpha
/// tracks: exact at the keys.
#[test]
fn each_layer_wears_its_bench_colour_alpha_and_blend() {
    // Just inside the window's close: newborns of both layers are live, and
    // so are particles at mid-life.
    let (mut h, _, _) = played(0.69);
    let particles = h.particles();
    let mut keys_seen: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut layers_seen = BTreeSet::new();
    for p in &particles {
        let m = h.material_of(p.entity);
        layers_seen.insert(p.emitter);
        assert_eq!(m.alpha_mode, AlphaMode::Add, "layer {} blends", p.emitter);
        assert!(!m.unlit, "an additive layer is lit emissive");
        assert!(m.base_color_texture.is_some() && m.emissive_texture.is_some());
        let lin = m.base_color.to_linear();
        assert!(
            (m.emissive.red - lin.red * GLOW_GAIN).abs() < 1e-5
                && (m.emissive.green - lin.green * GLOW_GAIN).abs() < 1e-5
                && (m.emissive.blue - lin.blue * GLOW_GAIN).abs() < 1e-5,
            "layer {} glows {:?} over {:?}",
            p.emitter,
            m.emissive,
            lin
        );
        // The palette entry it wears is the track at its step.
        let t = (p.age / BENCH[p.emitter].life).clamp(0.0, 1.0);
        let step = (t * 16.0).round();
        let ts = step / 16.0;
        let layer = &BENCH[p.emitter];
        let expected_alpha = track(layer.alpha, ts);
        assert!(
            (m.base_color.alpha() - expected_alpha).abs() < 1e-5,
            "layer {} at step {step}: alpha {}, bench {expected_alpha}",
            p.emitter,
            m.base_color.alpha()
        );
        if step == 0.0 || step == 8.0 || step == 16.0 {
            keys_seen.insert((p.emitter, step as usize));
            let key = layer.color[(step / 8.0) as usize];
            assert_eq!(rgb(m.base_color), key, "layer {} at key {step}", p.emitter);
        }
    }
    assert_eq!(layers_seen, BTreeSet::from([FLARES, GLOW]));
    // Both layers were caught on their birth and mid-life keys.
    for key in [(FLARES, 0), (FLARES, 8), (GLOW, 0), (GLOW, 8)] {
        assert!(keys_seen.contains(&key), "key {key:?} never drawn");
    }
}

/// Every palette entry's colour, for every step, is the bench track — a
/// middle colour too, not only the keys.
#[test]
fn the_colour_ramp_between_the_keys_is_the_bench_ramp() {
    let (mut h, _, _) = played(0.8);
    let particles = h.particles();
    let mut checked = 0;
    for p in &particles {
        let m = h.material_of(p.entity);
        let layer = &BENCH[p.emitter];
        let t = (p.age / layer.life).clamp(0.0, 1.0);
        let ts = (t * 16.0).round() / 16.0;
        let want: [f32; 3] = std::array::from_fn(|c| {
            track(
                [
                    layer.color[0][c] as f32,
                    layer.color[1][c] as f32,
                    layer.color[2][c] as f32,
                ],
                ts,
            )
        });
        let got = m.base_color.to_srgba();
        for (c, g) in [got.red, got.green, got.blue].into_iter().enumerate() {
            let w = want[c] / 255.0;
            // Exact where the bench value is a clear fraction; one step near
            // an integer.
            let tol = if (0.02..=0.98).contains(&w) {
                1e-4
            } else {
                1.0 / 255.0
            };
            assert!(
                (g - w).abs() <= tol,
                "layer {} channel {c} at t {ts}: {g}, bench {w}",
                p.emitter
            );
        }
        checked += 1;
    }
    assert!(checked > 40, "only {checked} particles checked");
}

/// Each quad is the unit quad scaled to the bench's diameter along its life.
#[test]
fn each_quad_is_sized_along_the_bench_scale_track() {
    let (mut h, _, _) = played(0.6);
    let particles = h.particles();
    assert!(particles.len() > 30);
    let mut layers = BTreeSet::new();
    for p in &particles {
        let mesh = h.mesh_of(p.entity);
        let aabb = mesh.compute_aabb().unwrap();
        assert!(
            (aabb.half_extents.x - 0.5).abs() < 1e-6 && (aabb.half_extents.y - 0.5).abs() < 1e-6,
            "the shared quad is 1 x 1: {:?}",
            aabb.half_extents
        );
        let layer = &BENCH[p.emitter];
        let radius = track(layer.scale, p.age / layer.life);
        assert!(
            (p.scale.x - 2.0 * radius).abs() < 1e-4 && (p.scale.y - 2.0 * radius).abs() < 1e-4,
            "layer {} at age {}: quad {:?}, radius {radius}",
            p.emitter,
            p.age,
            p.scale
        );
        layers.insert(p.emitter);
    }
    assert_eq!(layers, BTreeSet::from([FLARES, GLOW]));
}

/// The textures read back as the bench's sprites, pixel by pixel: the flare a
/// four-point star with a core, the glow a soft cloud.
#[test]
fn the_sprites_are_the_benchs_star_and_cloud() {
    let (mut h, _, _) = played(0.3);
    let particles = h.particles();
    for layer in [FLARES, GLOW] {
        let p = particles
            .iter()
            .find(|p| p.emitter == layer)
            .expect("both layers are live");
        let m = h.material_of(p.entity);
        let image = h.image(m.base_color_texture.as_ref().unwrap());
        let size = image.texture_descriptor.size.width;
        let data = image.data.as_ref().expect("the sprite has pixels");
        let mut lit = 0;
        for y in 0..size {
            for x in 0..size {
                let dx = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let dy = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let (r, angle) = ((dx * dx + dy * dy).sqrt(), dy.atan2(dx));
                let want = if layer == FLARES {
                    bench_star(r, angle)
                } else {
                    bench_cloud(r)
                };
                let got = data[((y * size + x) * 4 + 3) as usize] as f32 / 255.0;
                assert!(
                    (got - want.clamp(0.0, 1.0)).abs() <= 1.0 / 255.0 + 1e-6,
                    "layer {layer} pixel ({x},{y}) r {r}: {got}, bench {want}"
                );
                if got > 0.0 {
                    lit += 1;
                }
            }
        }
        assert!(lit > 100, "layer {layer}: only {lit} lit pixels");
    }
    // The two sprites differ where the shapes do: off the star's rays and
    // outside its core, the star is dark and the cloud is not.
    let at = |r: f32, a: f32| (bench_star(r, a), bench_cloud(r));
    let (star, cloud) = at(0.6, PI / 4.0);
    assert_eq!(star, 0.0);
    assert!(cloud > 0.3);
}

// ── in front of the body ─────────────────────────────────────────────────────

/// At the match camera, from every side, every particle is drawn nearer the
/// camera than the body's nearest point, on its own view ray, scaled by the
/// same ratio — so it is never painted over by the body and covers the
/// bench's screen footprint.
#[test]
fn every_particle_is_drawn_in_front_of_the_body() {
    // The match camera from every side, then the pitches and zooms its
    // controller allows either side of it.
    let views = [0.0, 1.3, 2.6, 4.0, 5.5]
        .map(|yaw| (60.0, 38.7, yaw))
        .into_iter()
        .chain([
            (60.0, 15.0, 0.5),
            (60.0, 60.0, 3.0),
            (60.0, 80.0, 5.0),
            (25.0, 38.7, 1.0),
            (100.0, 50.0, 2.0),
        ]);
    for (zoom, pitch, yaw) in views {
        let mut h = Harness::new();
        let at = Vec3::new(1.0, COMBATANT_Y, 2.0);
        let cam = match_camera(at, zoom, f32::to_radians(pitch), yaw);
        h.camera(cam);
        let victim = h.victim(at, vec![]);
        h.land(victim, AuraLandingKind::AftermathFireHit);
        h.tick_secs(0.5);
        let eye = cam.translation;
        let forward = cam.forward().as_vec3();
        let body_near = [
            at - Vec3::Y * BODY_HALF_SEGMENT,
            at + Vec3::Y * BODY_HALF_SEGMENT,
        ]
        .iter()
        .map(|p| (*p - eye).dot(forward))
        .fold(f32::MAX, f32::min)
            - BODY_RADIUS;
        let particles = h.particles();
        let mut held = 0;
        for p in &particles {
            let depth = (p.drawn - eye).dot(forward);
            assert!(
                depth < body_near,
                "{zoom} yd, pitch {pitch}, yaw {yaw}: drawn at depth {depth}, the body starts \
                 at {body_near}"
            );
            // On its own view ray.
            let ray = (p.truly - eye).normalize();
            let drawn_ray = (p.drawn - eye).normalize();
            assert!(ray.dot(drawn_ray) > 1.0 - 1e-6, "yaw {yaw}: off its ray");
            // Scaled by the ratio it was slid in.
            let k = depth / (p.truly - eye).dot(forward);
            let layer = &BENCH[p.emitter];
            let radius = track(layer.scale, p.age / layer.life);
            assert!(
                (p.scale.x - 2.0 * radius * k).abs() < 1e-4,
                "yaw {yaw}: quad {} for radius {radius} at k {k}",
                p.scale.x
            );
            if k < 0.999 {
                held += 1;
            }
        }
        assert!(
            held > 10,
            "{zoom} yd, pitch {pitch}, yaw {yaw}: only {held} of {} particles needed holding",
            particles.len()
        );
    }
}

/// How close in front of the body's nearest point a held particle stands
/// (`BODY_HOLD_CLEARANCE`, the shared hold).
const HOLD_CLEARANCE: f32 = 0.05;

/// The depth a particle at true depth `truly` is drawn at, in front of a body
/// whose nearest point stands at depth `body_near`: where it is if already
/// in front, else just in front of the body.
fn held_depth(truly: f32, body_near: f32) -> f32 {
    truly.min(body_near - HOLD_CLEARANCE)
}

/// The hold is judged against where the body IS: a victim that runs at the
/// camera under its burst (Aftermath is a slow, and the slowed run) still
/// has every particle drawn in front of it.
#[test]
fn the_burst_stays_in_front_of_a_victim_running_at_the_camera() {
    let at = Vec3::new(1.0, COMBATANT_Y, 2.0);
    let cam = match_camera(at, 60.0, 38.7f32.to_radians(), 0.0);
    let mut h = Harness::new();
    h.camera(cam);
    let victim = h.victim(at, vec![]);
    h.land(victim, AuraLandingKind::AftermathFireHit);
    h.tick_secs(0.1);
    let eye = cam.translation;
    let forward = cam.forward().as_vec3();
    let toward = Vec3::new(-forward.x, 0.0, -forward.z).normalize();
    // Half a second at 7 yd/s.
    let mut now = at;
    for _ in 0..31 {
        now += toward * 7.0 * TICK_SECS;
        h.move_to(victim, now);
        h.tick(1);
    }
    let body_near = [
        now - Vec3::Y * BODY_HALF_SEGMENT,
        now + Vec3::Y * BODY_HALF_SEGMENT,
    ]
    .iter()
    .map(|p| (*p - eye).dot(forward))
    .fold(f32::MAX, f32::min)
        - BODY_RADIUS;
    let particles = h.particles();
    assert!(particles.len() > 20, "{} particles", particles.len());
    let mut held = 0;
    for p in &particles {
        let truly = (p.truly - eye).dot(forward);
        let drawn = (p.drawn - eye).dot(forward);
        assert!(
            (drawn - held_depth(truly, body_near)).abs() < 1e-3,
            "drawn at depth {drawn}, held against the moved body it stands at {}",
            held_depth(truly, body_near)
        );
        if drawn < truly - 1e-3 {
            held += 1;
        }
    }
    assert!(held > 10, "only {held} particles needed holding");
}

/// A pet's burst is held against the PET's body — the sphere bounding its
/// laid-down capsule, centred 0.45 below its sim position, 0.35 + 0.3 in
/// radius — from every side: each particle is drawn just in front of that
/// sphere, on its own view ray.
#[test]
fn a_pets_burst_is_held_in_front_of_the_pet() {
    const PET_BOUND: f32 = 0.35 + 0.3;
    for yaw in [0.0f32, 2.0, 4.5] {
        let at = Vec3::new(-1.0, PET_SIM_Y, 4.0);
        let centre = at + Vec3::Y * PET_BODY_Y;
        let cam = match_camera(centre, 60.0, 38.7f32.to_radians(), yaw);
        let mut h = Harness::new();
        h.camera(cam);
        let pet = h.pet_victim(at);
        h.land(pet, AuraLandingKind::AftermathFireHit);
        h.tick_secs(0.4);
        let eye = cam.translation;
        let forward = cam.forward().as_vec3();
        let body_near = (centre - eye).dot(forward) - PET_BOUND;
        let particles = h.particles();
        assert!(particles.len() > 20, "yaw {yaw}: {}", particles.len());
        let mut held = 0;
        for p in &particles {
            let truly = (p.truly - eye).dot(forward);
            let drawn = (p.drawn - eye).dot(forward);
            assert!(
                (drawn - held_depth(truly, body_near)).abs() < 1e-3,
                "yaw {yaw}: drawn at depth {drawn}, held against the pet it stands at {}",
                held_depth(truly, body_near)
            );
            if drawn < truly - 1e-3 {
                held += 1;
            }
        }
        // The pet's chest is inside its bounding sphere, so at this burst's
        // reach every particle needs the hold: none is left where it is.
        assert!(held > 20, "yaw {yaw}: only {held} held");
    }
}

/// Every sprite faces the camera, from any view, and wears its own roll in
/// the camera's plane (the bench's `ang`), spread all the way round.
#[test]
fn every_sprite_faces_the_camera_at_its_own_roll() {
    for (pitch, yaw) in [(38.7f32, 0.0f32), (38.7, 2.2), (20.0, 4.0), (70.0, 1.0)] {
        let mut h = Harness::new();
        let at = Vec3::new(1.0, COMBATANT_Y, 2.0);
        let cam = match_camera(at, 60.0, pitch.to_radians(), yaw);
        h.camera(cam);
        let victim = h.victim(at, vec![]);
        h.land(victim, AuraLandingKind::AftermathFireHit);
        h.tick_secs(0.4);
        let world = h.app.world_mut();
        let drawn: Vec<(f32, Quat)> = world
            .query::<(&AftermathHitParticle, &GlobalTransform)>()
            .iter(world)
            .map(|(p, g)| (p.angle, g.to_scale_rotation_translation().1))
            .collect();
        assert!(drawn.len() > 20, "pitch {pitch} yaw {yaw}: {}", drawn.len());
        let (back, right, up) = (
            cam.rotation * Vec3::Z,
            cam.rotation * Vec3::X,
            cam.rotation * Vec3::Y,
        );
        let mut quadrants = BTreeSet::new();
        for (angle, rotation) in drawn {
            assert!(
                (rotation * Vec3::Z).dot(back) > 1.0 - 1e-5,
                "pitch {pitch} yaw {yaw}: a sprite does not face the camera"
            );
            let x = rotation * Vec3::X;
            let roll = x.dot(up).atan2(x.dot(right));
            let off = (roll - angle).rem_euclid(TAU);
            assert!(
                off < 1e-3 || off > TAU - 1e-3,
                "pitch {pitch} yaw {yaw}: rolled {roll}, its own roll is {angle}"
            );
            quadrants.insert((roll.rem_euclid(TAU) / (PI / 2.0)) as u8);
        }
        assert_eq!(
            quadrants,
            BTreeSet::from([0, 1, 2, 3]),
            "pitch {pitch} yaw {yaw}: rolls are not spread round"
        );
    }
}

// ── lifecycle ────────────────────────────────────────────────────────────────

/// The hit is the landing, not the state: the slow running out, being
/// dispelled or being refreshed mid-burst leaves it to play out beside the
/// victim's other auras (another slow, a root, Immolate), and a refresh's own
/// landing plays a second hit.
#[test]
fn the_slows_life_does_not_touch_a_hit_in_flight() {
    let others = || {
        vec![
            slow("Frostbolt", 0.6),
            Aura {
                effect_type: AuraType::Root,
                duration: 4.0,
                ability_name: "Frost Nova".into(),
                ..Default::default()
            },
            Aura {
                effect_type: AuraType::DamageOverTime,
                duration: 15.0,
                magnitude: 10.0,
                tick_interval: 3.0,
                ability_name: "Immolate".into(),
                ..Default::default()
            },
        ]
    };
    for case in ["expires", "dispelled", "refreshed"] {
        let mut h = Harness::new();
        let mut auras = others();
        auras.push(slow(AFTERMATH_AURA, 0.7));
        let victim = h.victim(Vec3::new(0.0, COMBATANT_Y, 0.0), auras);
        h.land(victim, AuraLandingKind::AftermathFireHit);
        h.tick_secs(0.4);
        let first = h.rigs()[0].0;
        let mut a = h.app.world_mut().get_mut::<ActiveAuras>(victim).unwrap();
        match case {
            // Expiry and a dispel both take the aura off its bearer.
            "expires" | "dispelled" => a.auras.retain(|x| x.ability_name != AFTERMATH_AURA),
            _ => {
                for x in a
                    .auras
                    .iter_mut()
                    .filter(|x| x.ability_name == AFTERMATH_AURA)
                {
                    x.duration = 5.0;
                }
            }
        }
        if case == "refreshed" {
            h.land(victim, AuraLandingKind::AftermathFireHit);
        }
        h.tick_secs(0.4);
        let rigs: Vec<Entity> = h.rigs().iter().map(|r| r.0).collect();
        assert!(rigs.contains(&first), "{case}: the first hit plays on");
        assert_eq!(
            rigs.len(),
            if case == "refreshed" { 2 } else { 1 },
            "{case}"
        );
        assert!(
            h.particles().iter().any(|p| p.emitter == FLARES),
            "{case}: still emitting"
        );
        // ...and retires on its own clock.
        h.tick_secs(0.95);
        let rigs: Vec<Entity> = h.rigs().iter().map(|r| r.0).collect();
        assert!(!rigs.contains(&first), "{case}: the first hit retired");
    }
}

/// A victim dying under the hit leaves it to play out: the hit already
/// happened.
#[test]
fn a_victim_that_dies_under_the_hit_keeps_it() {
    let (mut h, victim, _) = played(0.3);
    h.set_health(victim, 0.0);
    h.tick_secs(0.5);
    assert_eq!(h.rigs().len(), 1);
    assert!(h.count::<AftermathHitParticle>() > 0);
    h.tick_secs(1.0);
    assert!(h.rigs().is_empty(), "and retires on its own clock");
}

#[test]
fn a_victim_that_is_gone_takes_the_hit_with_it() {
    let (mut h, victim, _) = played(0.3);
    assert!(h.count::<AftermathHitParticle>() > 0);
    h.app.world_mut().despawn(victim);
    h.tick(1);
    assert!(h.rigs().is_empty());
    assert_eq!(h.count::<AftermathHitParticle>(), 0);
}

/// A polymorph hides the burst (the sheep has no chest at this height) and
/// its clock runs on; the hit shows again when the sheep is gone.
#[test]
fn a_sheep_hides_the_hit_and_its_clock_runs_on() {
    let (mut h, victim, _) = played(0.2);
    assert_eq!(h.rigs()[0].3, Visibility::Inherited);
    h.app
        .world_mut()
        .entity_mut(victim)
        .insert(PolymorphedVisual);
    h.tick(1);
    assert_eq!(h.rigs()[0].3, Visibility::Hidden);
    h.tick_secs(0.3);
    h.app
        .world_mut()
        .entity_mut(victim)
        .remove::<PolymorphedVisual>();
    h.tick(1);
    assert_eq!(h.rigs()[0].3, Visibility::Inherited);
    // The clock ran: it retires at 1.7 s from the landing, not later.
    h.tick_secs(1.2);
    assert!(h.rigs().is_empty());
}

/// A match that ends mid-burst takes all of it.
#[test]
fn the_match_ending_takes_the_hit() {
    let (mut h, _, _) = played(0.4);
    assert!(h.count::<AftermathHitParticle>() > 0);
    h.app
        .world_mut()
        .run_system_once(cleanup_play_match)
        .unwrap();
    h.tick(1);
    assert!(h.rigs().is_empty());
    assert_eq!(h.count::<AftermathHitParticle>(), 0);
}

// ── the ring ─────────────────────────────────────────────────────────────────

const RING_TICK: Duration = Duration::from_millis(20);
const RING_DT: f32 = 0.02;

struct RingHarness {
    app: App,
}

impl RingHarness {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(RING_TICK));
        app.add_systems(
            Update,
            (
                update_walk_animation,
                update_slow_treatment,
                update_slow_rings,
                update_slow_scuffs,
            )
                .chain(),
        );
        app.update();
        RingHarness { app }
    }

    fn unit(&mut self, auras: Vec<Aura>) -> Entity {
        let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
        let body = self
            .app
            .world_mut()
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        let unit = self
            .app
            .world_mut()
            .spawn((
                Transform::from_translation(at),
                Combatant::new(2, 0, CharacterClass::Warrior),
                WalkAnim {
                    phase: 0.0,
                    previous_xz: at.xz(),
                    idle_time: 1.0,
                    body_offset: 0.0,
                },
                ActiveAuras { auras },
            ))
            .id();
        self.app.world_mut().entity_mut(unit).add_child(body);
        unit
    }

    fn walk(&mut self, e: Entity, frames: usize) {
        for _ in 0..frames {
            let mut t = self.app.world_mut().get_mut::<Transform>(e).unwrap();
            t.translation += Vec3::X * 3.5 * RING_DT;
            self.app.update();
        }
    }

    fn auras(&mut self, e: Entity) -> Mut<'_, ActiveAuras> {
        self.app.world_mut().get_mut::<ActiveAuras>(e).unwrap()
    }

    /// The tints of the core rings born on the last frame, read off the
    /// material the renderer draws.
    fn newest_core_tints(&mut self) -> Vec<[u8; 3]> {
        let world = self.app.world_mut();
        let found: Vec<Handle<StandardMaterial>> = world
            .query::<(&SlowBindRing, &MeshMaterial3d<StandardMaterial>)>()
            .iter(world)
            .filter(|(r, _)| !r.halo && r.age < RING_DT * 1.5)
            .map(|(_, m)| m.0.clone())
            .collect();
        let materials = world.resource::<Assets<StandardMaterial>>();
        found
            .iter()
            .map(|h| rgb(materials.get(h).unwrap().base_color))
            .collect()
    }

    fn rings(&mut self) -> usize {
        let world = self.app.world_mut();
        world.query::<&SlowBindRing>().iter(world).count()
    }

    /// Walk until a fresh pulse is born, and return its core's tint.
    fn next_pulse_tint(&mut self, e: Entity) -> Option<[u8; 3]> {
        for _ in 0..60 {
            self.walk(e, 1);
            if let Some(t) = self.newest_core_tints().first() {
                return Some(*t);
            }
        }
        None
    }
}

#[test]
fn the_fire_tint_is_the_fire_schools_colour() {
    assert_eq!(SlowTint::Fire.color(), SpellSchool::Fire.color());
    assert_eq!(rgb(SlowTint::Fire.color()), FIRE_RGB);
}

#[test]
fn an_aftermath_victim_walks_in_the_fire_ring_and_stands_in_nothing() {
    let mut h = RingHarness::new();
    let unit = h.unit(vec![slow(AFTERMATH_AURA, 0.7)]);
    for _ in 0..20 {
        h.app.update();
    }
    assert_eq!(h.rings(), 0, "a standing victim wears no ring");
    assert_eq!(h.next_pulse_tint(unit), Some(FIRE_RGB));
}

/// Beside a root, Immolate and another slow, the ring takes the hardest
/// slow's tint; it turns Fire when the harder slow ends, keeps Fire through a
/// refresh, and stops with the slow — expired or dispelled.
#[test]
fn the_fire_ring_through_the_slows_life_beside_other_auras() {
    for ending in ["expires", "dispelled"] {
        let mut h = RingHarness::new();
        let unit = h.unit(vec![
            Aura {
                effect_type: AuraType::DamageOverTime,
                duration: 15.0,
                magnitude: 10.0,
                tick_interval: 3.0,
                ability_name: "Immolate".into(),
                ..Default::default()
            },
            slow("Frostbolt", 0.5),
            slow(AFTERMATH_AURA, 0.7),
        ]);
        let frost = rgb(SpellSchool::Frost.color());
        assert_eq!(h.next_pulse_tint(unit), Some(frost), "the harder slow wins");
        h.auras(unit)
            .auras
            .retain(|a| a.ability_name != "Frostbolt");
        assert_eq!(
            h.next_pulse_tint(unit),
            Some(FIRE_RGB),
            "{ending}: Fire now"
        );
        // A refresh keeps it.
        for a in h
            .auras(unit)
            .auras
            .iter_mut()
            .filter(|a| a.ability_name == AFTERMATH_AURA)
        {
            a.duration = 5.0;
        }
        assert_eq!(
            h.next_pulse_tint(unit),
            Some(FIRE_RGB),
            "{ending}: refreshed"
        );
        // A weaker slow beside it does not take the ring.
        h.auras(unit).auras.push(slow("Crippling Poison", 0.9));
        assert_eq!(
            h.next_pulse_tint(unit),
            Some(FIRE_RGB),
            "{ending}: weaker slow"
        );
        h.auras(unit)
            .auras
            .retain(|a| a.ability_name != "Crippling Poison");
        // The slow ends: no new pulse, though the root and the DoT stay.
        h.auras(unit)
            .auras
            .retain(|a| a.ability_name != AFTERMATH_AURA);
        h.auras(unit).auras.push(Aura {
            effect_type: AuraType::Root,
            duration: 4.0,
            ability_name: "Frost Nova".into(),
            ..Default::default()
        });
        assert_eq!(h.next_pulse_tint(unit), None, "{ending}: no ring after");
    }
}

#[test]
fn a_dead_victim_wears_no_ring() {
    let mut h = RingHarness::new();
    let unit = h.unit(vec![slow(AFTERMATH_AURA, 0.7)]);
    h.app
        .world_mut()
        .get_mut::<Combatant>(unit)
        .unwrap()
        .current_health = 0.0;
    assert_eq!(h.next_pulse_tint(unit), None);
}

// ── at the match camera ──────────────────────────────────────────────────────

/// The scale reads at the match camera: a newborn flare and a peak glow puff
/// both span several pixels at 1080p, and the burst stays within about two
/// yards of the chest.
#[test]
fn the_hit_reads_at_the_match_camera() {
    let px_per_yd = 1080.0 / (2.0 * 60.0 * (22.5f32.to_radians()).tan());
    assert!(BENCH[FLARES].scale[0] * 2.0 * px_per_yd > 8.0);
    assert!(BENCH[GLOW].scale[1] * 2.0 * px_per_yd > 15.0);
    let (mut h, _, at) = played(1.0);
    let far = h
        .particles()
        .iter()
        .map(|p| p.truly.distance(chest(at)))
        .fold(0.0, f32::max);
    assert!(far < 2.0, "the burst reaches {far} yd from the chest");
}

/// The `VisualBody` a unit's client spawns under it, at its rest height.
fn add_body(app: &mut App, unit: Entity, rest_y: f32) -> Entity {
    let body = app
        .world_mut()
        .spawn((
            arenasim::states::play_match::components::VisualBody { rest_y },
            Transform::from_xyz(0.0, rest_y, 0.0),
        ))
        .id();
    app.world_mut().entity_mut(unit).add_child(body);
    body
}

/// Where a body point `height` yards above the floor stands on a drawn body
/// whose root is `root` lifted `lift` off its rest pose: a pet's body hangs
/// 0.45 below its root at 0.55 of a combatant's stature.
fn drawn_point(root: Vec3, lift: f32, height: f32, pet: bool) -> Vec3 {
    let local = if pet {
        -0.45 + (height - 1.0) * 0.55
    } else {
        height - 1.0
    };
    root + Vec3::Y * (lift + local)
}

/// The bob and the lift `frame` frames in: the gait's 0.1 yd bob, then a
/// Cyclone's 1.5 yd lift from frame 30.
fn bob_and_lift(frame: u32, dt: f32) -> (Vec3, f32) {
    let t = frame as f32 * dt;
    let run = Vec3::new(2.0 * (t * 2.5).sin(), 0.0, 1.5 * (t * 2.3).sin());
    (
        run,
        0.1 * (t * 9.0).sin() + if frame >= 30 { 1.5 } else { 0.0 },
    )
}

/// Aftermath's fire hit rides its victim's DRAWN body, not its sim root: every
/// frame, as a combatant or a pet runs, its `VisualBody` bobbing on the gait
/// and then lifted 1.5 yd as a Cyclone lifts one, the rig stands at the drawn
/// body's chest.
#[test]
fn the_burst_rides_the_drawn_body() {
    for pet in [false, true] {
        let mut h = Harness::new();
        let (root_y, rest) = if pet {
            (PET_SIM_Y, PET_BODY_Y)
        } else {
            (COMBATANT_Y, 0.0)
        };
        let start = Vec3::new(1.0, root_y, -2.0);
        let victim = if pet {
            h.pet_victim(start)
        } else {
            h.victim(start, vec![])
        };
        let body = add_body(&mut h.app, victim, rest);
        h.land(victim, AuraLandingKind::AftermathFireHit);
        // Into the emit window: a rig with no particle yet has nothing to
        // stand.
        h.tick(10);
        for frame in 0..60 {
            let (run, lift) = bob_and_lift(frame, TICK_SECS);
            let root = start + run;
            h.app
                .world_mut()
                .get_mut::<Transform>(victim)
                .unwrap()
                .translation = root;
            h.app
                .world_mut()
                .get_mut::<Transform>(body)
                .unwrap()
                .translation
                .y = rest + lift;
            h.tick(1);
            let rigs = h.rigs();
            assert_eq!(rigs.len(), 1, "pet {pet} frame {frame}");
            assert!(
                h.particles().len() > 5,
                "pet {pet} frame {frame}: the burst is on"
            );
            let want = drawn_point(root, lift, 1.45, pet);
            assert!(
                rigs[0].2.distance(want) < 1e-4,
                "pet {pet} frame {frame}: the burst at {}, the drawn chest {want}",
                rigs[0].2
            );
        }
    }
}
