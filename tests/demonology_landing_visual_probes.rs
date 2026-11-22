//! Probes for Soul Link's rune column and Demonic Sacrifice's triad
//! (`rendering/effects/demonology_landings.rs`, AS-350).
//!
//! Three halves:
//!
//! - **The triggers.** The sacrifice, run through the shipped mechanic
//!   (`demonic_bonds`) and `apply_pending_auras` on a bare `World`, marks a
//!   Touch of Shadow landing on the Warlock, which owns its application (no
//!   aura band); Master Demonologist marks none. Soul Link's column asks
//!   `soul_link_forms`, read off a Warlock resolved from the shipped tree: the
//!   Felhunter choice forms the link, the Sacrifice choice does not.
//! - **The renderer**, on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin`
//!   with the chain `states/mod.rs` registers (and the shared particle ager
//!   and billboard), asserting what is DRAWN: the column's eleven flat
//!   additive rings at the feet (mesh, colour, blend, the stagger, the alpha
//!   ladder, the scale snap, the rise, the lifetime), its sparks (count,
//!   window, cone, area, speed, spin, palette), the triad's three orbs at the
//!   chest (glow texture, colour, radius and drift, spin), their ribbons
//!   (width, taper, trail length), the inner and outer motes (rate ramp,
//!   windows, birth points, colour ramps to the green end), and the edges: the
//!   gates opening once, nothing on a corpse, a Warlock's death mid-effect, a
//!   polymorph, other auras and other landings in flight, the
//!   `PlayMatchEntity` tag.
//! - **The match camera**: sizes as they read at 60 yd, pitch 38.7°, 45° FOV.
//!
//! Every bench number is a literal here, transcribed from
//! `docs/design/benches/2026-10-17-warlock-talent-bench.html` (the "Soul
//! Link" and "Demonic Sacrifice" tabs), never read from the module, so a
//! mutant of the module's constants cannot carry the expectation with it.

use std::collections::BTreeSet;
use std::f32::consts::{PI, TAU};
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::render::camera::CameraProjection;
use bevy::render::mesh::VertexAttributeValues;
use bevy::render::view::NoFrustumCulling;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::headless::HeadlessMatchConfig;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::auras::apply_pending_auras;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraApplyOwner, AuraApplyRoute, AuraLanding,
    AuraLandingKind, AuraType, ClientParticle, Combatant, EmpoweredFlash, GameRng, MatchCountdown,
    ParticleFacing, Pet, PetType, PlayMatchEntity, PolymorphedVisual,
};
use arenasim::states::play_match::constants::PET_SLOT_BASE;
use arenasim::states::play_match::demonology::{
    demonic_bonds, master_demonologist_aura, touch_of_shadow_aura,
};
use arenasim::states::play_match::talent_choices::SlotChoices;
use arenasim::states::play_match::talent_mechanics::{TalentMechanic, TalentMechanics};
use arenasim::states::play_match::talents::{Build, TalentDefinitions, TalentId};
use arenasim::states::play_match::{
    animate_client_particles, animate_empowered_flashes, animate_sacrifice_triads,
    animate_soul_link_columns, billboard_hunter_shots, hold_demonology_particles, soul_link_forms,
    spawn_demonology_landings, spawn_empowered_flashes, star_flash_texture,
    unhold_demonology_particles, BodyHeldParticle, SacrificeOrb, SacrificeRibbon, SacrificeTriad,
    SoulLinkColumn, SoulLinkRune,
};

// ── The bench, as literals ───────────────────────────────────────────────────

/// The Soul Link tab's `customDraw`: eleven rings from 334 ms, 100 ms apart.
const RUNES: usize = 11;
const FIRST_MS: f32 = 334.0;
const STAGGER_MS: f32 = 100.0;
/// `peak = i === 0 ? 1 : max(0.05, 0.45 - 0.04 * (i - 1))`.
const LADDER: [f32; 11] = [
    1.0, 0.45, 0.41, 0.37, 0.33, 0.29, 0.25, 0.21, 0.17, 0.13, 0.09,
];
/// `keys([[0,0],[80,peak],[850,peak],[1600+i*80,0]], t - t0)`.
const FADE_IN_MS: f32 = 80.0;
const HOLD_TO_MS: f32 = 850.0;
const GONE_MS: f32 = 1600.0;
const GONE_STEP_MS: f32 = 80.0;
/// `r = (1.76 - 0.05 i) * 0.5 * keys([[0,2.48],[200,0.91],[400,0.98]], t) / 0.98`.
const HALF_WIDTH: f32 = 1.76;
const HALF_WIDTH_STEP: f32 = 0.05;
/// `rise = keys([[0,0],[2500,5.06]], t) * (i / 10)`, on `y = 0.05 + rise`.
const RISE: f32 = 5.06;
const RING_FLOOR: f32 = 0.05;
/// `[255, i%3 ? 0 : 105, i%3 ? 0 : 105]`.
const RED: [f32; 3] = [255.0, 0.0, 0.0];
const PINK: [f32; 3] = [255.0, 105.0, 105.0];
/// The column's last ring has faded: 334 + 1000 + 1600 + 800 ms.
const COLUMN_GONE: f32 = 3.734;
/// The sparks: 56/s for 0-1000 ms, 2.4 yd/s, vRange 0.6, area 0.8, spin 3,
/// life 1.0, colour and scale over life.
const SPARK_RATE: f32 = 56.0;
const SPARK_SPEED: f32 = 2.4;
const SPARK_CONE: f32 = 0.6;
const SPARK_AREA: f32 = 0.8;
const SPARK_SPIN: f32 = 3.0;
const SPARK_COLORS: [[f32; 3]; 3] = [
    [255.0, 190.0, 170.0],
    [255.0, 40.0, 40.0],
    [255.0, 150.0, 140.0],
];
const SPARK_RADII: [f32; 2] = [0.08, 0.01];

/// The Demonic Sacrifice tab: three orbs at the chest (1.45), radius
/// `0.83 + 0.28 sin(2.7 t)`, angle `t × 0.94 × TAU + k × TAU / 3`.
const CHEST: f32 = 1.45;
const ORB_R: f32 = 0.83;
const ORB_DRIFT: f32 = 0.28;
const ORB_DRIFT_RATE: f32 = 2.7;
const ORB_REV: f32 = 0.94;
const ORB_BOB: f32 = 0.1;
const ORB_BOB_RATE: f32 = 4.0;
const ORB_GLOW: f32 = 0.35;
const ORB_RGB: [f32; 3] = [150.0, 120.0, 255.0];
/// `keys([[0,0],[334,0.8],[1000,0.8],[2367,0]], t)`, nothing after 2367 ms.
const ORB_ALPHA: [(f32, f32); 4] = [(0.0, 0.0), (334.0, 0.8), (1000.0, 0.8), (2367.0, 0.0)];
const TRIAD_GONE: f32 = 2.367;
/// One ribbon per orb: 0.33 yd wide, over the last 0.65 s, in 16 steps.
const RIBBON_W: f32 = 0.33;
const RIBBON_SECS: f32 = 0.65;
const RIBBON_STEPS: usize = 16;
/// The inner motes: rate `[[0,0],[334,100],[1000,100],[2367,0]]`, life 0.6,
/// speed 0.83, scale 0 -> 0.07 -> 0, alpha 0 -> 1 -> 0, and `colorMap` of
/// (0,30,255) -> (107,142,255) -> (126,172,255).
const INNER_LIFE: f32 = 0.6;
const INNER_SPEED: f32 = 0.83;
const INNER_SOURCE: [[f32; 3]; 3] = [
    [0.0, 30.0, 255.0],
    [107.0, 142.0, 255.0],
    [126.0, 172.0, 255.0],
];
/// The outer motes: 50/s for 1000-1667 ms at 1.75 yd, area 0.42, gravity
/// -6.94, life 0.8, scale 0.07 -> 0.03 -> 0.01, alpha 1 -> 0, and `colorMap`
/// of (220,229,246) -> (105,140,255) -> (0,25,213), green under g 60.
const OUTER_R: f32 = 1.75;
const OUTER_AREA: f32 = 0.42;
const OUTER_GRAVITY: f32 = -6.94;
const OUTER_LIFE: f32 = 0.8;
const OUTER_SOURCE: [[f32; 3]; 3] = [
    [220.0, 229.0, 246.0],
    [105.0, 140.0, 255.0],
    [0.0, 25.0, 213.0],
];
const GREEN: [f32; 3] = [25.0, 213.0, 0.0];
/// The house glow over an additive colour, and the shared palette's steps.
const GLOW: f32 = 2.5;
const STEPS: usize = 8;

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

fn ramp(keys: [[f32; 3]; 3], t: f32) -> [f32; 3] {
    let k = |c: usize| {
        keyed(
            &[(0.0, keys[0][c]), (0.5, keys[1][c]), (1.0, keys[2][c])],
            t,
        )
    };
    [k(0), k(1), k(2)]
}

/// The bench's Succubus `colorMap`.
fn violet(c: [f32; 3]) -> [f32; 3] {
    [(c[0] * 0.9 + c[2] * 0.35).min(255.0), c[1] * 0.8, c[2]]
}

fn outer_color(t: f32) -> [f32; 3] {
    let c = ramp(OUTER_SOURCE, t);
    if c[1] < 60.0 && c[2] > 150.0 {
        GREEN
    } else {
        violet(c)
    }
}

/// Ring `i`'s alpha `ms` into the landing.
fn ring_alpha(i: usize, ms: f32) -> f32 {
    let t0 = FIRST_MS + STAGGER_MS * i as f32;
    if ms < t0 {
        return 0.0;
    }
    keyed(
        &[
            (0.0, 0.0),
            (FADE_IN_MS, LADDER[i]),
            (HOLD_TO_MS, LADDER[i]),
            (GONE_MS + GONE_STEP_MS * i as f32, 0.0),
        ],
        ms - t0,
    )
}

fn ring_radius(i: usize, ms: f32) -> f32 {
    (HALF_WIDTH - HALF_WIDTH_STEP * i as f32)
        * 0.5
        * keyed(&[(0.0, 2.48), (200.0, 0.91), (400.0, 0.98)], ms)
        / 0.98
}

fn ring_height(i: usize, ms: f32) -> f32 {
    RING_FLOOR + keyed(&[(0.0, 0.0), (2500.0, RISE)], ms) * i as f32 / 10.0
}

// ── The triggers ─────────────────────────────────────────────────────────────

fn bare_world(gates: bool) -> World {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(GameRng::from_seed(7));
    world.insert_resource(Time::<()>::default());
    world.insert_resource(MatchCountdown {
        time_remaining: if gates { 0.0 } else { 5.0 },
        gates_opened: gates,
    });
    world
}

fn landings(world: &mut World) -> Vec<(Entity, AuraLandingKind)> {
    let mut q = world.query::<&AuraLanding>();
    q.iter(world).map(|l| (l.target, l.kind)).collect()
}

const SACRIFICE: TalentMechanic = TalentMechanic::DemonicSacrifice {
    shadow_damage: 0.15,
};
const DEMONOLOGIST: TalentMechanic = TalentMechanic::MasterDemonologist { reduction: 0.1 };

/// The sacrifice through the shipped mechanic: Master Demonologist's aura
/// lands before the gates and marks nothing; at the gates the Felhunter dies
/// and Touch of Shadow lands, marking the triad on the Warlock.
#[test]
fn the_sacrifice_marks_the_triad_on_the_warlock_and_master_demonologist_marks_nothing() {
    let mut world = bare_world(false);
    let mut warlock = Combatant::new(1, 0, CharacterClass::Warlock);
    warlock.talent_mechanics = TalentMechanics::of(&[DEMONOLOGIST, SACRIFICE]);
    warlock.sacrifice_pending = true;
    let owner = world.spawn((warlock, Transform::default())).id();
    let pet = world
        .spawn((
            Combatant::new(1, PET_SLOT_BASE, CharacterClass::Warlock),
            Pet {
                owner,
                pet_type: PetType::Felhunter,
            },
            Transform::default(),
        ))
        .id();

    // Before the gates: Master Demonologist on both, no landing.
    world.run_system_once(demonic_bonds).unwrap();
    world.run_system_once(apply_pending_auras).unwrap();
    assert!(world.get::<ActiveAuras>(owner).is_some());
    assert!(
        landings(&mut world).is_empty(),
        "Master Demonologist is icon-only"
    );

    // The gates: the sacrifice lands Touch of Shadow, marking the triad.
    world.resource_mut::<MatchCountdown>().gates_opened = true;
    world.run_system_once(demonic_bonds).unwrap();
    world.run_system_once(apply_pending_auras).unwrap();
    assert!(!world.get::<Combatant>(pet).unwrap().is_alive());
    assert_eq!(
        landings(&mut world),
        vec![(owner, AuraLandingKind::SacrificeTriad)]
    );

    // A later frame lands nothing more.
    world.run_system_once(demonic_bonds).unwrap();
    world.run_system_once(apply_pending_auras).unwrap();
    assert_eq!(landings(&mut world).len(), 1);
}

/// The triad owns Touch of Shadow's application, so the shared aura band stays
/// off it; another damage-dealt buff, and Master Demonologist, keep the band.
#[test]
fn the_triad_owns_touch_of_shadow_and_other_buffs_keep_the_band() {
    let buff = touch_of_shadow_aura(0.15, Entity::from_raw(1));
    assert_eq!(buff.ability_name, "Touch of Shadow");
    assert_eq!(buff.effect_type, AuraType::DamageDealtIncrease);
    assert_eq!(
        AuraApplyRoute::for_instance(buff.effect_type, &buff.ability_name),
        AuraApplyRoute::Owned(AuraApplyOwner::AuraLanding)
    );
    assert_eq!(
        AuraLandingKind::for_aura(
            AuraType::DamageDealtIncrease,
            "A Damage Buff With No Landing"
        ),
        None
    );
    assert_eq!(
        AuraApplyRoute::for_instance(
            AuraType::DamageDealtIncrease,
            "A Damage Buff With No Landing"
        ),
        AuraApplyRoute::Band
    );
    let md = master_demonologist_aura(0.1, Entity::from_raw(1));
    assert_eq!(
        AuraLandingKind::for_aura(md.effect_type, &md.ability_name),
        None
    );
    // The name alone does not route: Touch of Shadow is a damage-dealt buff.
    assert_eq!(
        AuraLandingKind::for_aura(AuraType::DamageTakenReduction, "Touch of Shadow"),
        None
    );
}

/// A Warlock resolved from the shipped tree as its spawn resolves it.
fn resolved(build: Build, demon: &str) -> Combatant {
    let config = HeadlessMatchConfig {
        team1: vec!["Warlock".into()],
        team2: vec!["Warrior".into()],
        team1_builds: vec![build.into()],
        team1_warlock_demons: vec![demon.into()],
        ..Default::default()
    };
    let match_config = config.to_match_config().expect("a legal config");
    let build = match_config.team1_builds[0].clone();
    let choices = SlotChoices::from_config(&match_config, 1, 0, CharacterClass::Warlock);
    let resolved = TalentDefinitions::shipped()
        .resolve_with_choices(
            AbilityDefinitions::shipped(),
            CharacterClass::Warlock,
            &build,
            &choices,
        )
        .expect("resolves");
    let mut combatant = Combatant::new(1, 0, CharacterClass::Warlock);
    resolved.apply(&mut combatant);
    combatant
}

fn linked_build() -> Build {
    Build::of(&[
        (TalentId::DemonicEmbrace, 2),
        (TalentId::FelStamina, 2),
        (TalentId::MasterDemonologist, 1),
        (TalentId::DemonicSacrifice, 1),
        (TalentId::SoulLink, 1),
    ])
}

/// The link forms on a shipped Soul Link Warlock keeping its Felhunter; the
/// Sacrifice choice switches it off, so no column; nor does a dead Felhunter,
/// a severed link or an untalented Warlock.
#[test]
fn the_link_forms_with_the_felhunter_and_not_with_the_sacrifice() {
    let felhunter = resolved(linked_build(), "Felhunter");
    assert!(soul_link_forms(&felhunter, true));
    assert!(!soul_link_forms(&felhunter, false), "its demon is dead");
    let mut severed = felhunter.clone();
    severed.soul_link_severed = true;
    assert!(!soul_link_forms(&severed, true));
    let mut dead = felhunter.clone();
    dead.current_health = 0.0;
    dead.is_dead = true;
    assert!(!soul_link_forms(&dead, true));

    let sacrifice = resolved(linked_build(), "Sacrifice");
    assert!(
        !soul_link_forms(&sacrifice, true),
        "Sacrifice switches the link off"
    );
    let plain = Combatant::new(1, 0, CharacterClass::Warlock);
    assert!(!soul_link_forms(&plain, true));
}

// ── The renderer ─────────────────────────────────────────────────────────────

const TICK: Duration = Duration::from_millis(16);
const TICK_SECS: f32 = 0.016;
/// Combatants stand with capsule centres at world y = 1.0 over the floor.
const COMBATANT_Y: f32 = 1.0;

struct Harness {
    app: App,
    camera: Option<Entity>,
}

impl Harness {
    /// No camera: every piece is drawn where it truly is (nothing to hold it
    /// in front of the body for), so the bench's geometry reads off exactly.
    fn new() -> Self {
        Self::build(None)
    }

    /// A camera at `at`: the pieces face it and are held in front of the body
    /// for it.
    fn with_camera(at: Transform) -> Self {
        Self::build(Some(at))
    }

    /// A camera turned well off every axis, so a sprite left unrotated (or
    /// turned to a fixed direction) cannot pass for one facing it.
    fn off_axis() -> Self {
        Self::with_camera(Transform::from_xyz(-23.0, 31.0, 17.0).looking_at(Vec3::ZERO, Vec3::Y))
    }

    fn build(camera_at: Option<Transform>) -> Self {
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
        app.insert_resource(MatchCountdown {
            time_remaining: 5.0,
            gates_opened: false,
        });
        // The empowered flash consumes the same marker type; it runs first and
        // must leave the triad's alone (and this chain must leave its alone).
        app.add_systems(
            Update,
            (spawn_empowered_flashes, animate_empowered_flashes)
                .chain()
                .before(spawn_demonology_landings),
        );
        // The order `states/mod.rs` registers, around the shared particle
        // ager, then the world-space billboard.
        app.add_systems(
            Update,
            (
                unhold_demonology_particles,
                spawn_demonology_landings,
                animate_soul_link_columns,
                animate_sacrifice_triads,
                animate_client_particles,
                hold_demonology_particles,
                billboard_hunter_shots,
            )
                .chain(),
        );
        let camera = camera_at.map(|t| app.world_mut().spawn((Camera3d::default(), t)).id());
        app.update();
        Harness { app, camera }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    /// Tick until the landing has run `secs`, given it began on a frame
    /// already ticked (age one tick).
    fn tick_to(&mut self, age_now: &mut f32, secs: f32) {
        while *age_now + TICK_SECS <= secs + 1e-4 {
            self.app.update();
            *age_now += TICK_SECS;
        }
    }

    /// A Warlock at `at` with `mechanics`, carrying other auras (Master
    /// Demonologist, and Touch of Shadow when sacrificing), and a Felhunter.
    fn warlock(&mut self, at: Vec3, mechanics: &[TalentMechanic], pet_alive: bool) -> Entity {
        let mut c = Combatant::new(1, 0, CharacterClass::Warlock);
        c.talent_mechanics = TalentMechanics::of(mechanics);
        let world = self.app.world_mut();
        let warlock = world.spawn((c, Transform::from_translation(at))).id();
        world.entity_mut(warlock).insert(ActiveAuras {
            auras: vec![master_demonologist_aura(0.1, warlock)],
        });
        let mut pet = Combatant::new(1, PET_SLOT_BASE, CharacterClass::Warlock);
        if !pet_alive {
            pet.current_health = 0.0;
            pet.is_dead = true;
        }
        world.spawn((
            pet,
            Pet {
                owner: warlock,
                pet_type: PetType::Felhunter,
            },
            Transform::from_translation(at + Vec3::X * 2.0),
        ));
        warlock
    }

    fn linked(&mut self, at: Vec3) -> Entity {
        self.warlock(at, &[TalentMechanic::SoulLink { share: 0.2 }], true)
    }

    fn open_gates(&mut self, open: bool) {
        let mut c = self.app.world_mut().resource_mut::<MatchCountdown>();
        c.gates_opened = open;
        c.time_remaining = if open { 0.0 } else { 5.0 };
    }

    fn land(&mut self, target: Entity, kind: AuraLandingKind) {
        self.app.world_mut().spawn((
            AuraLanding {
                target,
                kind,
                caster: Some(target),
            },
            PlayMatchEntity,
        ));
    }

    fn kill(&mut self, unit: Entity) {
        let mut c = self.app.world_mut().get_mut::<Combatant>(unit).unwrap();
        c.current_health = 0.0;
        c.is_dead = true;
    }

    fn move_to(&mut self, unit: Entity, at: Vec3) {
        self.app
            .world_mut()
            .get_mut::<Transform>(unit)
            .unwrap()
            .translation = at;
    }

    fn columns(&mut self) -> Vec<(Entity, Entity)> {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &SoulLinkColumn)>()
            .iter(world)
            .map(|(e, c)| (e, c.warlock))
            .collect()
    }

    fn runes(&mut self) -> Vec<(Entity, usize)> {
        let world = self.app.world_mut();
        let mut v: Vec<(Entity, usize)> = world
            .query::<(Entity, &SoulLinkRune)>()
            .iter(world)
            .map(|(e, r)| (e, r.index))
            .collect();
        v.sort_by_key(|(_, i)| *i);
        v
    }

    fn triads(&mut self) -> Vec<(Entity, Entity)> {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &SacrificeTriad)>()
            .iter(world)
            .map(|(e, t)| (e, t.warlock))
            .collect()
    }

    fn orbs(&mut self) -> Vec<(Entity, usize)> {
        let world = self.app.world_mut();
        let mut v: Vec<(Entity, usize)> = world
            .query::<(Entity, &SacrificeOrb)>()
            .iter(world)
            .map(|(e, o)| (e, o.index))
            .collect();
        v.sort_by_key(|(_, i)| *i);
        v
    }

    fn ribbons(&mut self) -> Vec<(Entity, usize)> {
        let world = self.app.world_mut();
        let mut v: Vec<(Entity, usize)> = world
            .query::<(Entity, &SacrificeRibbon)>()
            .iter(world)
            .map(|(e, r)| (e, r.index))
            .collect();
        v.sort_by_key(|(_, i)| *i);
        v
    }

    /// Every live particle: entity, world position, age, velocity.
    fn particles(&mut self) -> Vec<(Entity, Vec3, f32, Vec3)> {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &ClientParticle, &GlobalTransform)>()
            .iter(world)
            .filter(|(_, p, _)| p.age < p.life)
            .map(|(e, p, g)| (e, g.translation(), p.age, p.velocity))
            .collect()
    }

    fn all_particles(&mut self) -> usize {
        let world = self.app.world_mut();
        world.query::<&ClientParticle>().iter(world).count()
    }

    fn particle(
        &self,
        e: Entity,
    ) -> (f32, f32, [f32; 3], ParticleFacing, Handle<StandardMaterial>) {
        let p = self.app.world().get::<ClientParticle>(e).unwrap();
        let k = p.size.diameter_keys();
        (p.life, p.gravity, k, p.facing, p.palette[STEPS - 1].clone())
    }

    fn palette(&self, e: Entity) -> Vec<StandardMaterial> {
        let p = self.app.world().get::<ClientParticle>(e).unwrap();
        let mats = self.app.world().resource::<Assets<StandardMaterial>>();
        p.palette
            .iter()
            .map(|h| mats.get(h).unwrap().clone())
            .collect()
    }

    fn material(&self, part: Entity) -> StandardMaterial {
        let world = self.app.world();
        let handle = world
            .get::<MeshMaterial3d<StandardMaterial>>(part)
            .expect("part has a material")
            .0
            .clone();
        world
            .resource::<Assets<StandardMaterial>>()
            .get(&handle)
            .unwrap()
            .clone()
    }

    fn global(&self, part: Entity) -> GlobalTransform {
        *self.app.world().get::<GlobalTransform>(part).unwrap()
    }

    fn mesh(&self, part: Entity) -> Mesh {
        let world = self.app.world();
        let mesh = world
            .get::<Mesh3d>(part)
            .expect("part has a mesh")
            .0
            .clone();
        world.resource::<Assets<Mesh>>().get(&mesh).unwrap().clone()
    }

    fn image(&self, texture: &Handle<Image>) -> Image {
        self.app
            .world()
            .resource::<Assets<Image>>()
            .get(texture)
            .expect("texture")
            .clone()
    }
}

fn positions(mesh: &Mesh) -> Vec<Vec3> {
    let Some(VertexAttributeValues::Float32x3(ps)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("positions");
    };
    ps.iter().map(|p| Vec3::from(*p)).collect()
}

fn vertex_alpha(mesh: &Mesh) -> Vec<f32> {
    let Some(VertexAttributeValues::Float32x4(cs)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) else {
        panic!("vertex colours");
    };
    cs.iter().map(|c| c[3]).collect()
}

/// The alpha of `image` along its radius, at fractions `rs` of the half-width
/// from the centre (to the right of centre, on the middle row).
fn texture_alpha(image: &Image, rs: &[f32]) -> Vec<f32> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let data = image.data.as_ref().expect("texels");
    rs.iter()
        .map(|r| {
            let x = ((0.5 + 0.5 * r) * w as f32).min(w as f32 - 1.0) as usize;
            data[((h / 2) * w + x) * 4 + 3] as f32 / 255.0
        })
        .collect()
}

fn feet(at: Vec3) -> Vec3 {
    at - Vec3::Y * COMBATANT_Y
}

fn chest(at: Vec3) -> Vec3 {
    feet(at) + Vec3::Y * CHEST
}

fn srgb255(c: Color) -> [f32; 3] {
    let s = c.to_srgba();
    [s.red * 255.0, s.green * 255.0, s.blue * 255.0]
}

/// Colour compare (the card's rule): exact where the bench's value has a
/// fraction, within one unit where it sits on an integer — `tol` is the
/// float round trip, never a unit.
fn assert_rgb(c: Color, want: [f32; 3], what: &str) {
    let got = srgb255(c);
    for k in 0..3 {
        assert!(
            (got[k] - want[k]).abs() < 0.02,
            "{what}: rgb {got:?}, want {want:?}"
        );
    }
}

fn assert_glow(m: &StandardMaterial, what: &str) {
    assert_eq!(m.alpha_mode, AlphaMode::Add, "{what} is additive");
    assert!(!m.unlit, "{what}: unlit would discard the emissive");
    let c = m.base_color.to_linear();
    let e = m.emissive;
    assert!(
        (e.red - c.red * GLOW).abs() < 1e-4
            && (e.green - c.green * GLOW).abs() < 1e-4
            && (e.blue - c.blue * GLOW).abs() < 1e-4,
        "{what}: emissive {e:?} vs colour {c:?}"
    );
}

fn tagged(h: &Harness, e: Entity) -> bool {
    h.app.world().get::<PlayMatchEntity>(e).is_some()
}

/// A sprite's mesh is the unit quad: 1 yd on a side about its centre, flat in
/// its own XY, so its drawn size is its scale.
fn assert_unit_quad(mesh: &Mesh, what: &str) {
    let vs = positions(mesh);
    assert_eq!(vs.len(), 4, "{what}: a quad");
    for v in &vs {
        assert!(
            (v.x.abs() - 0.5).abs() < 1e-6 && (v.y.abs() - 0.5).abs() < 1e-6 && v.z.abs() < 1e-6,
            "{what}: vertex {v} of a unit quad"
        );
    }
}

// ── Soul Link ────────────────────────────────────────────────────────────────

/// The column forms once, as the gates open, on every Warlock whose link
/// forms and on no other: not before the gates, not on an unlinked Warlock,
/// one whose Felhunter is dead, one severed, nor one sacrificing.
#[test]
fn the_column_forms_once_at_the_gates_on_each_linked_warlock_only() {
    let mut h = Harness::new();
    let linked = h.linked(Vec3::new(0.0, COMBATANT_Y, 0.0));
    let second = h.linked(Vec3::new(10.0, COMBATANT_Y, 0.0));
    let _plain = h.warlock(Vec3::new(-6.0, COMBATANT_Y, 0.0), &[], true);
    let _orphan = h.warlock(
        Vec3::new(-6.0, COMBATANT_Y, 6.0),
        &[TalentMechanic::SoulLink { share: 0.2 }],
        false,
    );
    let severed = h.linked(Vec3::new(6.0, COMBATANT_Y, 6.0));
    h.app
        .world_mut()
        .get_mut::<Combatant>(severed)
        .unwrap()
        .soul_link_severed = true;
    let pending = h.linked(Vec3::new(6.0, COMBATANT_Y, -6.0));
    h.app
        .world_mut()
        .get_mut::<Combatant>(pending)
        .unwrap()
        .sacrifice_pending = true;

    h.tick(10);
    assert!(h.columns().is_empty(), "nothing before the gates");
    h.open_gates(true);
    h.tick(1);
    let owners: BTreeSet<Entity> = h.columns().iter().map(|(_, w)| *w).collect();
    assert_eq!(owners, BTreeSet::from([linked, second]));
    assert_eq!(h.runes().len(), 2 * RUNES);
    for (column, _) in h.columns() {
        assert!(tagged(&h, column), "the rig is a PlayMatchEntity");
    }
    h.tick(30);
    assert_eq!(h.columns().len(), 2, "it forms once, not every frame");

    // The next match's gates raise it again.
    h.tick_to(&mut 0.5, COLUMN_GONE + 0.1);
    assert!(h.columns().is_empty());
    h.open_gates(false);
    h.tick(2);
    assert!(h.columns().is_empty());
    h.open_gates(true);
    h.tick(1);
    assert_eq!(h.columns().len(), 2);
}

/// Eleven flat additive rings at the feet: each a band round its radius with
/// eight ticks from 0.82 of it out, lying in the ground plane; pure red, every
/// third from the first pink; lit by the house glow.
#[test]
fn the_column_is_eleven_flat_additive_rune_rings_at_the_feet() {
    let mut h = Harness::new();
    let at = Vec3::new(4.0, COMBATANT_Y, -3.0);
    let warlock = h.linked(at);
    h.open_gates(true);
    h.tick(1);
    let mut age = TICK_SECS;
    h.tick_to(&mut age, 0.5);
    assert_eq!(h.columns().len(), 1);
    let (column, owner) = h.columns()[0];
    assert_eq!(owner, warlock);
    let ms = age * 1000.0;

    let runes = h.runes();
    assert_eq!(
        runes.iter().map(|(_, i)| *i).collect::<Vec<_>>(),
        (0..RUNES).collect::<Vec<_>>()
    );
    for (rune, i) in runes {
        let g = h.global(rune);
        let (scale, rotation, translation) = g.to_scale_rotation_translation();
        assert!(rotation.angle_between(Quat::IDENTITY) < 1e-4, "flat");
        let want = feet(at) + Vec3::Y * ring_height(i, ms);
        assert!(
            translation.distance(want) < 1e-3,
            "ring {i}: {translation} vs {want}"
        );

        // The mesh: flat, a band round the settled radius, eight ticks.
        let mesh = h.mesh(rune);
        let vs = positions(&mesh);
        assert!(vs.iter().all(|v| v.y.abs() < 1e-6), "the ring lies flat");
        let settled = (HALF_WIDTH - HALF_WIDTH_STEP * i as f32) * 0.5;
        let radii: Vec<f32> = vs.iter().map(|v| Vec2::new(v.x, v.z).length()).collect();
        let outer = radii.iter().cloned().fold(0.0, f32::max);
        assert!(
            (outer - settled).abs() < 0.06,
            "ring {i}: rim {outer} vs {settled}"
        );
        // Exactly eight ticks: sixteen feet at 0.82 r, two per tick, their
        // midpoints on the eight compass angles k·TAU/8.
        let feet_at: Vec<Vec3> = vs
            .iter()
            .copied()
            .filter(|v| (Vec2::new(v.x, v.z).length() / settled - 0.82).abs() < 0.01)
            .collect();
        assert_eq!(feet_at.len(), 16, "ring {i}: tick feet at 0.82 r");
        let angles: BTreeSet<i32> = feet_at
            .chunks(2)
            .map(|pair| {
                let mid = (pair[0] + pair[1]) * 0.5;
                let a = mid.z.atan2(mid.x).rem_euclid(TAU);
                (a / (TAU / 8.0)).round() as i32 % 8
            })
            .collect();
        assert_eq!(
            angles,
            (0..8).collect::<BTreeSet<i32>>(),
            "ring {i}: tick angles"
        );
        for pair in feet_at.chunks(2) {
            let mid = (pair[0] + pair[1]) * 0.5;
            let a = mid.z.atan2(mid.x).rem_euclid(TAU);
            let k = (a / (TAU / 8.0)).round();
            assert!(
                (a - k * TAU / 8.0).abs() < 1e-3,
                "ring {i}: a tick at {a} rad"
            );
        }
        // Drawn at its bone scale.
        let drawn = settled * scale.x;
        assert!(
            (drawn - ring_radius(i, ms)).abs() < 1e-3,
            "ring {i}: {drawn}"
        );

        let m = h.material(rune);
        assert_glow(&m, "a ring");
        assert!(m.base_color_texture.is_none(), "a flat stroke");
        assert_rgb(
            m.base_color,
            if i % 3 == 0 { PINK } else { RED },
            "ring colour",
        );
    }
    assert!(tagged(&h, column));
}

/// The rings fade in 100 ms apart from 334 ms, each to its rung of the
/// ladder (1.0, then 0.45 falling by 0.04), hold to 850 ms of their own time
/// and fade by 1600 + 80 i; the first is still unborn at 330 ms.
#[test]
fn the_rings_fade_in_100_ms_apart_down_the_ladder() {
    let mut h = Harness::new();
    let _w = h.linked(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.open_gates(true);
    h.tick(1);
    let mut age = TICK_SECS;
    let mut peaks = [0.0f32; RUNES];
    let mut first_lit = [f32::MAX; RUNES];
    while age < COLUMN_GONE - 0.05 {
        let ms = age * 1000.0;
        for (rune, i) in h.runes() {
            let a = h.material(rune).base_color.alpha();
            let want = ring_alpha(i, ms);
            assert!(
                (a - want).abs() < 1e-3,
                "ring {i} at {ms} ms: {a} vs {want}"
            );
            peaks[i] = peaks[i].max(a);
            if a > 0.0 && first_lit[i] == f32::MAX {
                first_lit[i] = ms;
            }
        }
        h.tick(1);
        age += TICK_SECS;
    }
    for i in 0..RUNES {
        assert!(
            (peaks[i] - LADDER[i]).abs() < 1e-3,
            "ring {i} peaks {}",
            peaks[i]
        );
        let start = FIRST_MS + STAGGER_MS * i as f32;
        assert!(
            first_lit[i] > start && first_lit[i] <= start + 17.0,
            "ring {i} lit at {} ms",
            first_lit[i]
        );
    }
}

/// The ring scale snaps 2.48 -> 0.91 at 200 ms -> 0.98 on the landing's
/// clock (over the settled 0.98), and the column rises: the top ring to
/// 5.06 yd at 2.5 s, each below `i / 10` of that, the bottom one never.
#[test]
fn the_column_snaps_its_scale_and_rises_five_yards() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let _w = h.linked(at);
    h.open_gates(true);
    h.tick(1);
    let mut age = TICK_SECS;
    let scale_of = |h: &Harness, rune: Entity| h.global(rune).to_scale_rotation_translation().0.x;
    // 16 ms: still near the 2.48 snap; 208 ms: past 0.91; 400 ms: settled.
    let (rune0, _) = h.runes()[0];
    let want = keyed(&[(0.0, 2.48), (200.0, 0.91), (400.0, 0.98)], 16.0) / 0.98;
    assert!((scale_of(&h, rune0) - want).abs() < 1e-3);
    h.tick_to(&mut age, 0.208);
    let want = keyed(&[(0.0, 2.48), (200.0, 0.91), (400.0, 0.98)], age * 1000.0) / 0.98;
    assert!(
        (scale_of(&h, rune0) - want).abs() < 1e-3,
        "{}",
        scale_of(&h, rune0)
    );
    assert!(scale_of(&h, rune0) < 0.95, "past the 0.91 snap");
    h.tick_to(&mut age, 0.416);
    assert!((scale_of(&h, rune0) - 1.0).abs() < 1e-4, "settled");

    h.tick_to(&mut age, 2.512);
    for (rune, i) in h.runes() {
        let y = h.global(rune).translation().y - feet(at).y;
        let want = RING_FLOOR + RISE * i as f32 / 10.0;
        assert!((y - want).abs() < 1e-3, "ring {i} at {y} vs {want}");
    }
}

/// The column retires once its last ring has faded (3.734 s), rings and all.
#[test]
fn the_column_retires_after_its_last_ring() {
    let mut h = Harness::new();
    let _w = h.linked(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.open_gates(true);
    h.tick(1);
    let mut age = TICK_SECS;
    h.tick_to(&mut age, COLUMN_GONE - 0.04);
    assert_eq!(h.columns().len(), 1);
    assert_eq!(h.runes().len(), RUNES);
    let (last, _) = h.runes()[RUNES - 1];
    assert!(
        h.material(last).base_color.alpha() > 0.0,
        "the last ring still fading"
    );
    h.tick_to(&mut age, COLUMN_GONE + 0.02);
    assert!(h.columns().is_empty());
    assert!(h.runes().is_empty());
}

/// The sparks: 56 over the first second and none after, each born in the
/// 0.8 yd square at the feet (and filling it), launched at 2.4 yd/s within
/// 0.6 rad of straight up (and filling the cone), turning at 3 rev/s, living
/// 1 s at radius 0.08 -> 0.01, on the star sprite, additive, coloured
/// pale red -> red -> pink and fading out.
#[test]
fn the_sparks_spiral_out_of_the_feet_for_the_first_second() {
    let mut h = Harness::new();
    let at = Vec3::new(2.0, COMBATANT_Y, 5.0);
    let _w = h.linked(at);
    h.open_gates(true);
    h.tick(1);
    let mut age = TICK_SECS;
    let mut seen: BTreeSet<Entity> = BTreeSet::new();
    let mut births: Vec<(Vec3, Vec3)> = Vec::new();
    while age < 1.3 {
        for (e, pos, p_age, v) in h.particles() {
            if seen.insert(e) {
                // Back to its birth: no gravity, no drag.
                births.push((pos - v * p_age, v));
                assert!(tagged(&h, e), "a spark is a PlayMatchEntity");
            }
        }
        h.tick(1);
        age += TICK_SECS;
    }
    let n = births.len() as f32;
    assert!((SPARK_RATE - 1.0..=SPARK_RATE).contains(&n), "{n} sparks");
    // None born after the first second: by 1.3 s none is younger than 0.3 s.
    assert!(h.particles().iter().all(|(_, _, a, _)| *a >= 0.29));

    let half = SPARK_AREA / 2.0;
    let (mut max_x, mut max_z, mut max_pol) = (0.0f32, 0.0f32, 0.0f32);
    for (birth, v) in &births {
        let d = *birth - feet(at);
        assert!(d.y.abs() < 1e-3, "born on the floor: {d}");
        assert!(d.x.abs() <= half + 1e-3 && d.z.abs() <= half + 1e-3, "{d}");
        max_x = max_x.max(d.x.abs());
        max_z = max_z.max(d.z.abs());
        assert!((v.length() - SPARK_SPEED).abs() < 1e-3, "{}", v.length());
        let pol = v.normalize().angle_between(Vec3::Y);
        assert!(pol <= SPARK_CONE + 1e-3, "{pol} rad off up");
        max_pol = max_pol.max(pol);
    }
    assert!(
        max_x > half * 0.85 && max_z > half * 0.85,
        "the square is filled"
    );
    assert!(max_pol > SPARK_CONE * 0.85, "the cone is filled: {max_pol}");

    // One spark's make-up.
    let mut h = Harness::new();
    let _w = h.linked(at);
    h.open_gates(true);
    h.tick(3);
    let (spark, ..) = h.particles()[0];
    let (life, gravity, diam, facing, _) = h.particle(spark);
    assert_eq!(life, 1.0);
    assert_eq!(gravity, 0.0);
    assert!((diam[0] - 2.0 * SPARK_RADII[0]).abs() < 1e-5, "{diam:?}");
    assert!((diam[2] - 2.0 * SPARK_RADII[1]).abs() < 1e-5, "{diam:?}");
    match facing {
        ParticleFacing::Spin { mrad_per_sec, .. } => {
            let want = SPARK_SPIN * TAU * 1000.0;
            assert!((mrad_per_sec as f32 - want).abs() < 1.0, "{mrad_per_sec}");
        }
        other => panic!("a spark spins: {other:?}"),
    }
    assert_unit_quad(&h.mesh(spark), "a spark");
    let palette = h.palette(spark);
    assert_eq!(palette.len(), STEPS);
    let star = star_flash_texture();
    for (i, m) in palette.iter().enumerate() {
        let t = (i as f32 + 0.5) / STEPS as f32;
        assert_glow(m, "a spark");
        assert_rgb(m.base_color, ramp(SPARK_COLORS, t), "spark colour");
        assert!((m.base_color.alpha() - (1.0 - t)).abs() < 1e-4);
        let tex = h.image(m.base_color_texture.as_ref().expect("a sprite"));
        assert_eq!(tex.data, star.data, "the star sprite");
    }
}

// ── Demonic Sacrifice ────────────────────────────────────────────────────────

fn sacrificer(h: &mut Harness, at: Vec3) -> Entity {
    let w = h.warlock(at, &[DEMONOLOGIST, SACRIFICE], false);
    h.app
        .world_mut()
        .get_mut::<ActiveAuras>(w)
        .unwrap()
        .auras
        .push(touch_of_shadow_aura(0.15, w));
    w
}

/// The angle of a point about the chest axis, unwrapped against `near`.
fn angle_about(p: Vec3, centre: Vec3) -> f32 {
    let d = p - centre;
    d.z.atan2(d.x)
}

fn wrap(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

/// Touch of Shadow's landing raises one triad at the chest: three orbs a third
/// of a turn apart, 0.83 + 0.28 sin(2.7 t) yd out at the chest's height,
/// turning 0.94 rev/s; each a camera-facing violet glow 0.35 yd across on the
/// bench's soft sprite, additive.
#[test]
fn the_triad_is_three_violet_orbs_turning_round_the_chest() {
    let mut h = Harness::new();
    let at = Vec3::new(-3.0, COMBATANT_Y, 2.0);
    let warlock = sacrificer(&mut h, at);
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.tick(1);
    let mut age = TICK_SECS;
    h.tick_to(&mut age, 0.5);
    let triads = h.triads();
    assert_eq!(triads.len(), 1);
    assert_eq!(triads[0].1, warlock);
    assert!(tagged(&h, triads[0].0));
    let centre = chest(at);
    assert!(h.global(triads[0].0).translation().distance(centre) < 1e-4);

    let orbs = h.orbs();
    assert_eq!(
        orbs.iter().map(|(_, i)| *i).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let t = age;
    let mut angles = Vec::new();
    for (orb, k) in &orbs {
        let g = h.global(*orb);
        let (scale, rotation, p) = g.to_scale_rotation_translation();
        assert!((p.y - centre.y).abs() < 1e-4, "the glow sits at the chest");
        let r = Vec2::new(p.x - centre.x, p.z - centre.z).length();
        let want_r = ORB_R + ORB_DRIFT * (t * ORB_DRIFT_RATE).sin();
        assert!((r - want_r).abs() < 1e-3, "orb {k}: {r} vs {want_r}");
        let want_a = t * ORB_REV * TAU + *k as f32 * TAU / 3.0;
        let a = angle_about(p, centre);
        assert!(wrap(a - want_a).abs() < 1e-3, "orb {k}: {a} vs {want_a}");
        angles.push(a);
        assert!((scale.x - ORB_GLOW).abs() < 1e-4, "{}", scale.x);
        let _ = rotation;
        assert_unit_quad(&h.mesh(*orb), "an orb");
        let m = h.material(*orb);
        assert_glow(&m, "an orb");
        assert_rgb(m.base_color, ORB_RGB, "orb colour");
        let tex = h.image(m.base_color_texture.as_ref().expect("a glow sprite"));
        let falloff = texture_alpha(&tex, &[0.0, 0.45, 0.98]);
        assert!(falloff[0] > 0.97, "{falloff:?}");
        assert!((falloff[1] - 0.45).abs() < 0.03, "{falloff:?}");
        assert!(falloff[2] < 0.04, "{falloff:?}");
    }
    assert!((wrap(angles[1] - angles[0]) - TAU / 3.0).abs() < 1e-3);

    // A quarter second on, each has turned 0.94 × 0.25 of a turn.
    let before = angle_about(h.global(orbs[0].0).translation(), centre);
    h.tick_to(&mut age, 0.756);
    let after = angle_about(h.global(orbs[0].0).translation(), centre);
    let turned = wrap(after - before);
    let want = ORB_REV * TAU * (age - t);
    assert!((turned - want).abs() < 1e-3, "{turned} vs {want}");
}

/// The orbs and ribbons fade 0 -> 0.8 at 334 ms, hold to 1000 ms and fade to
/// nothing at 2367 ms, when the rig retires, orbs and ribbons with it.
#[test]
fn the_triad_fades_on_its_keys_and_retires_at_2367_ms() {
    let mut h = Harness::new();
    let warlock = sacrificer(&mut h, Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.tick(1);
    let mut age = TICK_SECS;
    while age < TRIAD_GONE - 0.02 {
        let want = keyed(&ORB_ALPHA, age * 1000.0);
        let (orb, _) = h.orbs()[0];
        let (ribbon, _) = h.ribbons()[0];
        let a = h.material(orb).base_color.alpha();
        assert!((a - want).abs() < 1e-3, "orb at {age}: {a} vs {want}");
        let r = h.material(ribbon).base_color.alpha();
        assert!((r - want).abs() < 1e-3, "ribbon at {age}: {r} vs {want}");
        h.tick(1);
        age += TICK_SECS;
    }
    assert_eq!(h.triads().len(), 1);
    h.tick_to(&mut age, TRIAD_GONE + 0.02);
    assert!(h.triads().is_empty());
    assert!(h.orbs().is_empty() && h.ribbons().is_empty());
}

/// One ribbon per orb: a camera-facing strip through the orb's bobbing path
/// over the last 0.65 s in 16 steps, 0.33 yd wide at the orb, its width and
/// vertex alpha tapering by `1 - s/16`; additive, flat violet.
#[test]
fn each_orb_trails_one_tapering_ribbon_over_0_65_s() {
    let mut h = Harness::new();
    let at = Vec3::new(1.0, COMBATANT_Y, 1.0);
    let warlock = sacrificer(&mut h, at);
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.tick(1);
    let mut age = TICK_SECS;
    h.tick_to(&mut age, 0.8);
    let ribbons = h.ribbons();
    assert_eq!(
        ribbons.iter().map(|(_, i)| *i).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let centre = chest(at);
    for (ribbon, k) in ribbons {
        // Rewritten every frame, so its first bounding box goes stale: it is
        // never frustum-culled.
        assert!(
            h.app.world().get::<NoFrustumCulling>(ribbon).is_some(),
            "a ribbon is never frustum-culled"
        );
        let m = h.material(ribbon);
        assert_glow(&m, "a ribbon");
        assert_rgb(m.base_color, ORB_RGB, "ribbon colour");
        let g = h.global(ribbon);
        let mesh = h.mesh(ribbon);
        let vs: Vec<Vec3> = positions(&mesh)
            .iter()
            .map(|v| g.transform_point(*v))
            .collect();
        assert_eq!(
            vs.len(),
            2 * (RIBBON_STEPS + 1),
            "17 samples, two edges each"
        );
        let alpha = vertex_alpha(&mesh);
        for s in 0..=RIBBON_STEPS {
            let taper = 1.0 - s as f32 / RIBBON_STEPS as f32;
            let (a, b) = (vs[2 * s], vs[2 * s + 1]);
            assert!(
                ((a - b).length() - RIBBON_W * taper).abs() < 1e-4,
                "width at {s}"
            );
            assert!((alpha[2 * s] - taper).abs() < 1e-5 && (alpha[2 * s + 1] - taper).abs() < 1e-5);
            // Centred on the orb's bobbing path at that sample's time.
            let tt = age - RIBBON_SECS * s as f32 / RIBBON_STEPS as f32;
            let r = ORB_R + ORB_DRIFT * (tt * ORB_DRIFT_RATE).sin();
            let ang = tt * ORB_REV * TAU + k as f32 * TAU / 3.0;
            let want = centre
                + Vec3::new(
                    r * ang.cos(),
                    ORB_BOB * (tt * ORB_BOB_RATE + k as f32).sin(),
                    r * ang.sin(),
                );
            assert!(
                ((a + b) * 0.5).distance(want) < 1e-3,
                "sample {s} of ribbon {k}"
            );
        }
    }

    // Early on the trail is only as long as the landing.
    let mut h = Harness::new();
    let warlock = sacrificer(&mut h, at);
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.tick(1);
    let mut age = TICK_SECS;
    h.tick_to(&mut age, 0.2);
    let (ribbon, _) = h.ribbons()[0];
    let samples = positions(&h.mesh(ribbon)).len() / 2;
    let want = (age / (RIBBON_SECS / RIBBON_STEPS as f32)).floor() as usize + 1;
    assert_eq!(samples, want);
}

/// A particle as first seen: the landing time it was emitted at (the end of
/// its emitting frame), its birth point and velocity, and its make-up.
#[derive(Clone)]
struct Birth {
    t: f32,
    pos: Vec3,
    v: Vec3,
    life: f32,
}

/// Every particle the run emits until `until` seconds into the landing (which
/// began one tick before this is called), each tagged `PlayMatchEntity`.
fn record_births(h: &mut Harness, until: f32) -> Vec<Birth> {
    let mut age = TICK_SECS;
    let mut seen: BTreeSet<Entity> = BTreeSet::new();
    let mut births = Vec::new();
    while age <= until {
        for (e, pos, p_age, v) in h.particles() {
            if seen.insert(e) {
                assert!(tagged(h, e), "a mote is a PlayMatchEntity");
                let life = h.app.world().get::<ClientParticle>(e).unwrap().life;
                // Aged and moved one tick in the frame that emitted it.
                births.push(Birth {
                    t: age - (p_age - TICK_SECS),
                    pos: pos - v * p_age,
                    v,
                    life,
                });
            }
        }
        h.tick(1);
        age += TICK_SECS;
    }
    births
}

/// A live particle of `life` at `secs` into a fresh triad.
fn live_mote(secs: f32, life: f32) -> (Harness, Entity) {
    let mut h = Harness::new();
    let warlock = sacrificer(&mut h, Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.tick(1);
    let mut age = TICK_SECS;
    h.tick_to(&mut age, secs);
    let e = h
        .particles()
        .into_iter()
        .map(|(e, ..)| e)
        .find(|e| h.particle(*e).0 == life)
        .expect("a live mote");
    (h, e)
}

/// The inner motes: about 152 over the landing on the bench's ramped rate
/// (16.7 by 334 ms, 66.6 more by 1000 ms, 68.4 more by 2367 ms), each born at
/// one of the three orbs (at 0.83 yd, all three used) at 0.83 yd/s in every
/// direction, living 0.6 s at radius 0 -> 0.07 -> 0 on the soft glow, in the
/// bench's violet map of the blue track, fading in and out.
#[test]
fn the_inner_motes_ramp_at_the_orbs() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let warlock = sacrificer(&mut h, at);
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.tick(1);
    let centre = chest(at);
    let inner: Vec<Birth> = record_births(&mut h, 3.2)
        .into_iter()
        .filter(|b| b.life == INNER_LIFE)
        .collect();
    let between = |a: f32, b: f32| inner.iter().filter(|m| m.t >= a && m.t < b).count() as f32;
    assert!(
        (between(0.0, 0.334) - 16.7).abs() <= 2.0,
        "{}",
        between(0.0, 0.334)
    );
    assert!(
        (between(0.334, 1.0) - 66.6).abs() <= 2.0,
        "{}",
        between(0.334, 1.0)
    );
    assert!(
        (between(1.0, 2.4) - 68.35).abs() <= 2.0,
        "{}",
        between(1.0, 2.4)
    );
    assert_eq!(between(2.4, 9.0), 0.0);

    let mut orbs_used = BTreeSet::new();
    let (mut up, mut down) = (false, false);
    for m in &inner {
        assert!((m.v.length() - INNER_SPEED).abs() < 1e-3);
        up |= m.v.y > INNER_SPEED * 0.8;
        down |= m.v.y < -INNER_SPEED * 0.8;
        assert!(
            (m.pos.y - centre.y).abs() < 1e-3,
            "born at the chest's height"
        );
        let d = Vec2::new(m.pos.x - centre.x, m.pos.z - centre.z);
        assert!(
            (d.length() - ORB_R).abs() < 1e-3,
            "born at the orbs' radius"
        );
        let a = d.y.atan2(d.x);
        let k = (0..3)
            .find(|k| wrap(a - (m.t * ORB_REV * TAU + *k as f32 * TAU / 3.0)).abs() < 0.01)
            .expect("born at one of the three orbs");
        orbs_used.insert(k);
    }
    assert_eq!(orbs_used, BTreeSet::from([0, 1, 2]));
    assert!(up && down, "every direction");

    let (h, e) = live_mote(0.5, INNER_LIFE);
    let (_, gravity, diam, facing, _) = h.particle(e);
    assert_eq!(gravity, 0.0);
    assert_eq!(facing, ParticleFacing::Camera);
    assert!(diam[0].abs() < 1e-6 && (diam[1] - 0.14).abs() < 1e-5 && diam[2].abs() < 1e-6);
    assert_unit_quad(&h.mesh(e), "an inner mote");
    let palette = h.palette(e);
    assert_eq!(palette.len(), STEPS);
    for (i, m) in palette.iter().enumerate() {
        let t = (i as f32 + 0.5) / STEPS as f32;
        assert_glow(m, "an inner mote");
        assert_rgb(
            m.base_color,
            violet(ramp(INNER_SOURCE, t)),
            "inner mote colour",
        );
        let want_a = keyed(&[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)], t);
        assert!((m.base_color.alpha() - want_a).abs() < 1e-4);
        let tex = h.image(m.base_color_texture.as_ref().unwrap());
        let falloff = texture_alpha(&tex, &[0.0, 0.45]);
        assert!(
            falloff[0] > 0.97 && (falloff[1] - 0.45).abs() < 0.03,
            "{falloff:?}"
        );
    }
}

/// The outer motes: about 33 between 1000 and 1667 ms and none outside, each
/// born still in a 0.42 yd square about one of three points 1.75 yd out at
/// the chest (filling it), rising under -6.94 yd/s², living 0.8 s at radius
/// 0.07 -> 0.03 -> 0.01 on the star, white through violet to the green end.
#[test]
fn the_outer_motes_rise_off_three_points_and_end_green() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let warlock = sacrificer(&mut h, at);
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.tick(1);
    let centre = chest(at);
    let outer: Vec<Birth> = record_births(&mut h, 3.2)
        .into_iter()
        .filter(|b| b.life == OUTER_LIFE)
        .collect();
    let n = outer.len() as f32;
    assert!((n - 50.0 * 0.667).abs() <= 1.5, "{n}");
    assert!(outer.iter().all(|m| m.t >= 1.0 && m.t < 1.667 + TICK_SECS));

    let half = OUTER_AREA / 2.0;
    let mut used = BTreeSet::new();
    let mut spread = 0.0f32;
    for m in &outer {
        // Born still: one tick of its gravity is all the speed it has.
        assert!(
            m.v.x.abs() < 1e-6 && m.v.z.abs() < 1e-6,
            "born still, rising"
        );
        assert!(
            (m.v.y - -OUTER_GRAVITY * TICK_SECS).abs() < 1e-3,
            "{}",
            m.v.y
        );
        assert!((m.pos.y - centre.y).abs() < 1e-3);
        let point = |k: usize| {
            let a = m.t * ORB_REV * TAU + k as f32 * TAU / 3.0;
            centre + Vec3::new(OUTER_R * a.cos(), 0.0, OUTER_R * a.sin())
        };
        let k = (0..3)
            .find(|k| {
                let p = point(*k);
                (m.pos.x - p.x).abs() <= half + 1e-3 && (m.pos.z - p.z).abs() <= half + 1e-3
            })
            .expect("born about one of the three outer points");
        used.insert(k);
        let p = point(k);
        spread = spread.max((m.pos.x - p.x).abs()).max((m.pos.z - p.z).abs());
    }
    assert_eq!(used, BTreeSet::from([0, 1, 2]));
    assert!(spread > half * 0.8, "the square is filled: {spread}");

    let (h, e) = live_mote(1.2, OUTER_LIFE);
    let (_, gravity, diam, facing, _) = h.particle(e);
    assert_eq!(gravity, OUTER_GRAVITY);
    assert_eq!(facing, ParticleFacing::Camera);
    assert!(
        (diam[0] - 0.14).abs() < 1e-5
            && (diam[1] - 0.06).abs() < 1e-5
            && (diam[2] - 0.02).abs() < 1e-5
    );
    let palette = h.palette(e);
    assert_eq!(palette.len(), STEPS);
    let star = star_flash_texture();
    for (i, m) in palette.iter().enumerate() {
        let t = (i as f32 + 0.5) / STEPS as f32;
        assert_glow(m, "an outer mote");
        assert_rgb(m.base_color, outer_color(t), "outer mote colour");
        assert!((m.base_color.alpha() - (1.0 - t)).abs() < 1e-4);
        assert_eq!(
            h.image(m.base_color_texture.as_ref().unwrap()).data,
            star.data
        );
    }
    assert_rgb(palette[STEPS - 1].base_color, GREEN, "the green end");
    assert_rgb(
        palette[0].base_color,
        violet(ramp(OUTER_SOURCE, 1.0 / 16.0)),
        "the white start",
    );
}

// ── Lifecycle ────────────────────────────────────────────────────────────────

/// Both rigs follow their Warlock; their particles stay where they were born.
#[test]
fn the_rigs_follow_the_warlock_and_their_particles_stay_put() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let warlock = h.linked(at);
    h.open_gates(true);
    h.tick(1);
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.tick(10);
    let (spark, before, ..) = h.particles()[0];
    let v = h.app.world().get::<ClientParticle>(spark).unwrap().velocity;
    let moved = Vec3::new(5.0, COMBATANT_Y, -4.0);
    h.move_to(warlock, moved);
    h.tick(1);
    let (column, _) = h.columns()[0];
    let (triad, _) = h.triads()[0];
    assert!(h.global(column).translation().distance(feet(moved)) < 1e-4);
    assert!(h.global(triad).translation().distance(chest(moved)) < 1e-4);
    let after = h.global(spark).translation();
    assert!(after.distance(before + v * TICK_SECS) < 1e-3, "world-space");
}

/// A Warlock dying mid-effect takes both rigs with it at once; their
/// world-space particles finish their short lives and go.
#[test]
fn a_warlock_dying_mid_effect_retires_both_rigs() {
    let mut h = Harness::new();
    let warlock = h.linked(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.open_gates(true);
    h.tick(1);
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.tick(30);
    assert_eq!(h.columns().len(), 1);
    assert_eq!(h.triads().len(), 1);
    assert!(h.all_particles() > 0);
    h.kill(warlock);
    h.tick(1);
    assert!(h.columns().is_empty() && h.runes().is_empty());
    assert!(h.triads().is_empty() && h.orbs().is_empty() && h.ribbons().is_empty());
    h.tick((1.0 / TICK_SECS) as u32 + 2);
    assert_eq!(h.all_particles(), 0, "the particles finish and go");
}

/// A polymorph does not stop a landing: both play out on the sheep and retire
/// on their own clocks.
#[test]
fn a_polymorphed_warlock_plays_both_out() {
    let mut h = Harness::new();
    let warlock = h.linked(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.open_gates(true);
    h.tick(1);
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.tick(20);
    h.app
        .world_mut()
        .entity_mut(warlock)
        .insert(PolymorphedVisual);
    h.app
        .world_mut()
        .get_mut::<ActiveAuras>(warlock)
        .unwrap()
        .auras
        .push(Aura {
            effect_type: AuraType::Polymorph,
            duration: 8.0,
            magnitude: 1.0,
            ability_name: "Polymorph".into(),
            ..Default::default()
        });
    h.tick(60);
    assert_eq!(h.columns().len(), 1);
    assert_eq!(h.triads().len(), 1);
    let mut age = 81.0 * TICK_SECS;
    h.tick_to(&mut age, TRIAD_GONE + 0.05);
    assert!(h.triads().is_empty());
    assert_eq!(h.columns().len(), 1);
    h.tick_to(&mut age, COLUMN_GONE + 0.05);
    assert!(h.columns().is_empty());
}

/// Nothing is drawn on a corpse, and the marker is consumed all the same.
#[test]
fn nothing_is_drawn_on_a_corpse() {
    let mut h = Harness::new();
    let warlock = h.linked(Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.kill(warlock);
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.open_gates(true);
    h.tick(3);
    assert!(h.columns().is_empty() && h.triads().is_empty());
    assert_eq!(h.all_particles(), 0);
    assert!(
        landings(h.app.world_mut()).is_empty(),
        "the marker is consumed"
    );
}

/// The triad's chain leaves another renderer's landing alone, and the other
/// renderer leaves the triad's: an Amplify Curse flash in the same frame plays
/// its own flash and no triad.
#[test]
fn another_renderers_landing_is_left_alone() {
    let mut h = Harness::new();
    let warlock = sacrificer(&mut h, Vec3::new(0.0, COMBATANT_Y, 0.0));
    h.land(warlock, AuraLandingKind::AmplifyFlash);
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.land(warlock, AuraLandingKind::ShadowWordPainHit);
    h.tick(1);
    assert_eq!(h.triads().len(), 1, "one triad");
    let flashes = {
        let world = h.app.world_mut();
        world
            .query_filtered::<Entity, With<EmpoweredFlash>>()
            .iter(world)
            .count()
    };
    assert_eq!(flashes, 1, "the flash is the empowered flash's");
    let left: Vec<AuraLandingKind> = landings(h.app.world_mut())
        .into_iter()
        .map(|(_, k)| k)
        .collect();
    assert_eq!(left, vec![AuraLandingKind::ShadowWordPainHit]);
}

// ── Through the default match camera ─────────────────────────────────────────

/// The match camera's first-frame placement (`camera.rs`): 60 yd out, 38.7°
/// off vertical, looking at the fight; the client's default 45° projection at
/// 1920 × 1080.
struct MatchCamera {
    clip_from_world: Mat4,
}

const SCREEN: Vec2 = Vec2::new(1920.0, 1080.0);

/// The match camera's transform, looking at `target`.
fn match_camera_at(target: Vec3) -> Transform {
    let pitch = 38.7f32.to_radians();
    let eye = target + Vec3::new(0.0, 60.0 * pitch.cos(), 60.0 * pitch.sin());
    Transform::from_translation(eye).looking_at(target, Vec3::Y)
}

impl MatchCamera {
    fn looking_at(target: Vec3) -> Self {
        let view = match_camera_at(target);
        let projection = PerspectiveProjection {
            aspect_ratio: SCREEN.x / SCREEN.y,
            ..default()
        };
        assert_eq!(
            projection.fov,
            PI / 4.0,
            "the client's camera is Bevy's default"
        );
        Self {
            clip_from_world: projection.get_clip_from_view() * view.compute_matrix().inverse(),
        }
    }

    fn pixel(&self, p: Vec3) -> Vec2 {
        let clip = self.clip_from_world * p.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        Vec2::new(
            (ndc.x + 1.0) * 0.5 * SCREEN.x,
            (1.0 - ndc.y) * 0.5 * SCREEN.y,
        )
    }

    fn span(&self, p: Vec3, width: f32) -> f32 {
        (self.pixel(p + Vec3::X * width * 0.5) - self.pixel(p - Vec3::X * width * 0.5)).length()
    }
}

/// At the match camera the rings read as rings (30-40 px across) with a stroke
/// of about 2 px, the top ring stays on screen, and the sparks, orbs and
/// ribbons are a few pixels: legible, and none a blob over the body.
#[test]
fn the_landings_read_at_the_match_camera() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let warlock = h.linked(at);
    h.open_gates(true);
    h.tick(1);
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    let mut age = TICK_SECS;
    h.tick_to(&mut age, 0.6);
    let cam = MatchCamera::looking_at(at);

    for (rune, i) in h.runes() {
        let g = h.global(rune);
        let r =
            (HALF_WIDTH - HALF_WIDTH_STEP * i as f32) * 0.5 * g.to_scale_rotation_translation().0.x;
        let px = cam.span(g.translation(), 2.0 * r);
        assert!((25.0..45.0).contains(&px), "ring {i} spans {px} px");
        // The stroke: the band's width on the mesh.
        let vs = positions(&h.mesh(rune));
        let stroke = (vs[1] - vs[0]).length();
        let spx = cam.span(g.translation(), stroke);
        assert!((1.7..2.3).contains(&spx), "ring {i} stroke {spx} px");
    }
    let top = feet(at) + Vec3::Y * (RING_FLOOR + RISE);
    let p = cam.pixel(top);
    assert!(
        p.y > 0.0 && p.y < SCREEN.y,
        "the top ring stays on screen: {p}"
    );

    for (e, pos, ..) in h.particles() {
        let size = h.global(e).to_scale_rotation_translation().0.x;
        let px = cam.span(pos, size);
        assert!(px < 5.0, "a particle draws {px} px");
    }
    let spark_birth = cam.span(feet(at), 2.0 * SPARK_RADII[0]);
    assert!(
        (2.5..5.0).contains(&spark_birth),
        "a new spark {spark_birth} px"
    );
    let orb = cam.span(chest(at), ORB_GLOW);
    assert!((5.0..10.0).contains(&orb), "an orb {orb} px");
    let ribbon = cam.span(chest(at), RIBBON_W);
    assert!((5.0..10.0).contains(&ribbon), "a ribbon {ribbon} px");
}

// ── Held in front of the body ────────────────────────────────────────────────
//
// The body is alpha-blended and writes no depth, so Bevy orders it against
// these additive pieces by each mesh's translation alone (view depth, back to
// front). A piece whose translation is deeper than the body's is drawn first
// and painted over by the body; the bench draws every piece over it.

/// View depth along the camera's forward axis: what Bevy's rangefinder sorts
/// transparent draws by.
fn depth(cam: &Transform, p: Vec3) -> f32 {
    (p - cam.translation).dot(cam.forward().as_vec3())
}

/// A Soul Link Warlock at `at` with both landings playing, under a camera at
/// `camera`.
fn both_landings(camera: Transform, at: Vec3) -> (Harness, Entity) {
    let mut h = Harness::with_camera(camera);
    let warlock = h.linked(at);
    h.open_gates(true);
    h.tick(1);
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    (h, warlock)
}

/// Every ring, orb, ribbon, spark and mote sorts in front of the Warlock's
/// body at the match camera, for the whole of both landings; the held pieces
/// stand in front of the whole body capsule (radius 0.5, segment ±0.75), so
/// the drawn body's bob, sink or bounce about its transform cannot reorder
/// them. And the run is not vacuous: low rings, far orbs, sparks and motes
/// alike truly stood deeper than the body and were held.
#[test]
fn every_piece_is_drawn_after_the_body_at_the_match_camera() {
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let cam = match_camera_at(at);
    let (mut h, _) = both_landings(cam, at);
    let body = depth(&cam, at);
    // The capsule's nearest point: the nearer end of its segment, less its
    // radius.
    let skin = depth(&cam, at + Vec3::Y * 0.75).min(depth(&cam, at - Vec3::Y * 0.75)) - 0.5;
    let mut age = TICK_SECS;
    let (mut runes_held, mut orbs_held, mut sparks_held, mut motes_held) = (0, 0, 0, 0);
    while age < COLUMN_GONE - 0.02 {
        let ms = age * 1000.0;
        // The triad began one tick after the column.
        let t = age - TICK_SECS;
        for (rune, i) in h.runes() {
            let d = depth(&cam, h.global(rune).translation());
            assert!(d < body, "ring {i} at {ms} ms sorts behind the body");
            if depth(&cam, feet(at) + Vec3::Y * ring_height(i, ms)) > body {
                runes_held += 1;
                assert!(d < skin, "a held ring clears the whole body");
            }
        }
        for (orb, k) in h.orbs() {
            let d = depth(&cam, h.global(orb).translation());
            assert!(d < body, "orb {k}");
            let r = ORB_R + ORB_DRIFT * (t * ORB_DRIFT_RATE).sin();
            let a = t * ORB_REV * TAU + k as f32 * TAU / 3.0;
            if depth(&cam, chest(at) + Vec3::new(r * a.cos(), 0.0, r * a.sin())) > body {
                orbs_held += 1;
                assert!(d < skin, "a held orb clears the whole body");
            }
        }
        for (ribbon, k) in h.ribbons() {
            assert!(
                depth(&cam, h.global(ribbon).translation()) < body,
                "ribbon {k}"
            );
        }
        let held: Vec<(Entity, Vec3, f32)> = {
            let world = h.app.world_mut();
            world
                .query::<(Entity, &BodyHeldParticle, &ClientParticle)>()
                .iter(world)
                .filter(|(_, _, p)| p.age < p.life)
                .map(|(e, b, p)| (e, b.at, p.life))
                .collect()
        };
        for (e, true_at, life) in held {
            let d = depth(&cam, h.global(e).translation());
            assert!(d < body, "a particle");
            if depth(&cam, true_at) > body {
                assert!(d < skin, "a held particle clears the whole body");
                if life == 1.0 {
                    sparks_held += 1;
                } else {
                    motes_held += 1;
                }
            }
        }
        h.tick(1);
        age += TICK_SECS;
    }
    assert!(runes_held > 0 && orbs_held > 0, "{runes_held} {orbs_held}");
    assert!(
        sparks_held > 0 && motes_held > 0,
        "{sparks_held} {motes_held}"
    );
}

/// Held, a piece keeps its pixels: drawn on its own view ray, scaled by the
/// ratio it was pulled in by, it covers exactly what the bench's geometry
/// would — rings, orbs, ribbons and particles alike.
#[test]
fn the_hold_keeps_every_piece_on_its_pixels() {
    let at = Vec3::new(2.0, COMBATANT_Y, -1.0);
    let camera = match_camera_at(at);
    let (mut h, _) = both_landings(camera, at);
    let cam = MatchCamera::looking_at(at);
    let right = camera.right().as_vec3();
    let mut age = TICK_SECS;
    h.tick_to(&mut age, 0.9);
    let ms = age * 1000.0;
    let t = age - TICK_SECS;
    let close = |a: Vec2, b: Vec2, what: &str| assert!(a.distance(b) < 0.05, "{what}: {a} vs {b}");

    for (rune, i) in h.runes() {
        let g = h.global(rune);
        let centre = feet(at) + Vec3::Y * ring_height(i, ms);
        let s = keyed(&[(0.0, 2.48), (200.0, 0.91), (400.0, 0.98)], ms) / 0.98;
        for v in positions(&h.mesh(rune)).iter().step_by(7) {
            close(
                cam.pixel(g.transform_point(*v)),
                cam.pixel(centre + *v * s),
                "a ring vertex",
            );
        }
    }
    for (orb, k) in h.orbs() {
        let g = h.global(orb);
        let r = ORB_R + ORB_DRIFT * (t * ORB_DRIFT_RATE).sin();
        let a = t * ORB_REV * TAU + k as f32 * TAU / 3.0;
        let truly = chest(at) + Vec3::new(r * a.cos(), 0.0, r * a.sin());
        let (scale, _, drawn) = g.to_scale_rotation_translation();
        close(cam.pixel(drawn), cam.pixel(truly), "an orb");
        let span = |p: Vec3, w: f32| {
            (cam.pixel(p + right * w * 0.5) - cam.pixel(p - right * w * 0.5)).length()
        };
        assert!(
            (span(drawn, scale.x) - span(truly, ORB_GLOW)).abs() < 0.05,
            "orb {k} size"
        );
    }
    for (ribbon, _) in h.ribbons() {
        let g = h.global(ribbon);
        for v in positions(&h.mesh(ribbon)) {
            close(
                cam.pixel(g.transform_point(v)),
                cam.pixel(chest(at) + v),
                "a ribbon vertex",
            );
        }
    }
    let held: Vec<(Entity, Vec3, f32, f32)> = {
        let world = h.app.world_mut();
        world
            .query::<(Entity, &BodyHeldParticle, &ClientParticle)>()
            .iter(world)
            .filter(|(_, _, p)| p.age < p.life && p.life == 1.0)
            .map(|(e, b, p)| (e, b.at, p.age, p.life))
            .collect()
    };
    assert!(!held.is_empty());
    for (e, truly, p_age, life) in held {
        let (scale, _, drawn) = h.global(e).to_scale_rotation_translation();
        close(cam.pixel(drawn), cam.pixel(truly), "a spark");
        let u = p_age / life;
        let diameter = keyed(&[(0.0, 0.16), (0.5, 0.09), (1.0, 0.02)], u);
        let k = depth(&camera, drawn) / depth(&camera, truly);
        assert!((scale.x - diameter * k).abs() < 1e-4, "a spark's size");
    }
}

/// The hold is for the draw only: a held spark moves on its true straight
/// path, and is drawn on that path's pixels.
#[test]
fn a_held_particle_moves_on_its_true_path() {
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let camera = match_camera_at(at);
    let (mut h, _) = both_landings(camera, at);
    let cam = MatchCamera::looking_at(at);
    h.tick(5);
    let world = h.app.world_mut();
    let (spark, at0, v) = world
        .query::<(Entity, &BodyHeldParticle, &ClientParticle)>()
        .iter(world)
        .find(|(_, b, p)| p.life == 1.0 && depth(&camera, b.at) > depth(&camera, at))
        .map(|(e, b, p)| (e, b.at, p.velocity))
        .expect("a spark born deeper than the body");
    h.tick(20);
    let now = h.app.world().get::<BodyHeldParticle>(spark).unwrap().at;
    let want = at0 + v * 20.0 * TICK_SECS;
    assert!(now.distance(want) < 1e-3, "{now} vs {want}");
    let drawn = h.global(spark).translation();
    assert!(drawn.distance(now) > 1e-3, "it is held");
    assert!(cam.pixel(drawn).distance(cam.pixel(now)) < 0.05);
}

/// The orbs and the ribbons face a camera turned well off every axis.
#[test]
fn the_orbs_and_ribbons_face_a_rotated_camera() {
    let mut h = Harness::off_axis();
    let warlock = sacrificer(&mut h, Vec3::new(1.0, COMBATANT_Y, 1.0));
    h.land(warlock, AuraLandingKind::SacrificeTriad);
    h.tick(1);
    let mut age = TICK_SECS;
    h.tick_to(&mut age, 0.8);
    let camera = h.global(h.camera.unwrap());
    let toward = camera.rotation() * Vec3::Z;
    for (orb, _) in h.orbs() {
        let rotation = h.global(orb).rotation();
        assert!(
            (rotation * Vec3::Z).dot(toward) > 0.999,
            "an orb faces the camera"
        );
    }
    let eye = camera.translation();
    for (ribbon, k) in h.ribbons() {
        let g = h.global(ribbon);
        let vs: Vec<Vec3> = positions(&h.mesh(ribbon))
            .iter()
            .map(|v| g.transform_point(*v))
            .collect();
        for s in 0..RIBBON_STEPS {
            let (a, b) = (vs[2 * s], vs[2 * s + 1]);
            let view = (eye - (a + b) * 0.5).normalize();
            assert!(
                (a - b).normalize().dot(view).abs() < 1e-3,
                "ribbon {k} at {s}"
            );
        }
    }
}
