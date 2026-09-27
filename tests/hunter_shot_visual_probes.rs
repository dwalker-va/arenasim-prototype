//! Probes for the Hunter shots (`rendering/effects/hunter_shots.rs`): the
//! client's shared violet shot, Serpent Sting's green one, their two landings,
//! and the Auto Shot arrow.
//!
//! These assert WORLD-SPACE geometry and the routing contract: that the three
//! shared shots land identically whatever their schools, that the landing is
//! the three layers the user kept, that the missile's particles trail behind
//! it and Serpent Sting's drip, that the sting's cloud lingers on a moving
//! victim, and that the arrow ends AT a target that ran — not where it stood.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window, no
//! GPU. `TransformPlugin` is load-bearing: without it `GlobalTransform` never
//! propagates and every assertion below would read a child's LOCAL pose.

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce as _;
use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::MeshAabb as _;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::{AbilityType, SpellSchool};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    BoltTrail, ClientParticle, Combatant, CosmeticArrow, HunterShotCore, ImpactAnchor,
    ImpactSprite, ParticleFacing, Projectile, SchoolImpact,
};
use arenasim::states::play_match::{
    animate_client_particles, animate_hunter_shots, animate_school_impacts, hunter_shot_for,
    impact_origin, impact_style, landing_style, particle_rotation, spawn_auto_shot_arrow,
    spawn_hunter_shot_visuals, spawn_school_impacts, update_cosmetic_arrows, AutoShotArrowAssets,
    HunterShotKind, AUTO_ARROW_LENGTH, AUTO_ARROW_SPEED, AUTO_ARROW_WIDTH,
    HUNTER_SHOT_CORE_DIAMETER,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;

fn app() -> App {
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
    app.add_systems(
        Update,
        (
            spawn_hunter_shot_visuals,
            animate_hunter_shots,
            update_cosmetic_arrows,
            spawn_school_impacts,
            animate_school_impacts,
            animate_client_particles,
        )
            .chain(),
    );
    app
}

fn spawn_victim(app: &mut App, at: Vec3) -> Entity {
    app.world_mut()
        .spawn((
            Combatant::new(0, 0, CharacterClass::Warrior),
            Transform::from_translation(at),
        ))
        .id()
}

fn land(app: &mut App, ability: AbilityType, victim: Entity) -> Entity {
    let school = AbilityDefinitions::default()
        .get(&ability)
        .expect("in the config")
        .spell_school;
    app.world_mut()
        .spawn(SchoolImpact {
            target: victim,
            ability,
            school,
            anchor: ImpactAnchor::Chest,
            from: Vec3::X,
            magnitude: 0.1,
            is_crit: false,
            age: 0.0,
        })
        .id()
}

/// Every client particle with its world position.
fn particles(app: &mut App) -> Vec<(Vec3, Vec3, f32, bool)> {
    let mut q = app
        .world_mut()
        .query::<(&ClientParticle, &GlobalTransform)>();
    q.iter(app.world())
        .filter(|(p, _)| p.age < p.life)
        .map(|(p, g)| (g.translation(), p.velocity, p.gravity, p.owned))
        .collect()
}

fn count<T: Component>(app: &mut App) -> usize {
    let mut q = app.world_mut().query_filtered::<Entity, With<T>>();
    q.iter(app.world()).count()
}

fn mean(points: &[Vec3]) -> Vec3 {
    points.iter().copied().sum::<Vec3>() / points.len().max(1) as f32
}

// ── routing ────────────────────────────────────────────────────────────────

#[test]
fn the_router_names_the_four_shots_and_nothing_else() {
    for a in [
        AbilityType::AimedShot,
        AbilityType::ArcaneShot,
        AbilityType::ConcussiveShot,
    ] {
        assert_eq!(hunter_shot_for(a), Some(HunterShotKind::Shared), "{a:?}");
    }
    assert_eq!(
        hunter_shot_for(AbilityType::SerpentSting),
        Some(HunterShotKind::SerpentSting)
    );
    // Every other projectile in the config keeps its own missile.
    let defs = AbilityDefinitions::default();
    let routed: BTreeSet<AbilityType> = defs
        .iter()
        .filter(|(_, c)| c.projectile_speed.is_some())
        .map(|(a, _)| *a)
        .filter(|a| hunter_shot_for(*a).is_some())
        .collect();
    assert_eq!(
        routed,
        BTreeSet::from([
            AbilityType::AimedShot,
            AbilityType::ArcaneShot,
            AbilityType::ConcussiveShot,
            AbilityType::SerpentSting,
        ])
    );
}

/// The RON tints are not a second colour authority: none of the four carries
/// one, so nothing can read a tint the shot does not draw.
#[test]
fn no_shot_carries_a_ron_tint() {
    let defs = AbilityDefinitions::default();
    for a in [
        AbilityType::AimedShot,
        AbilityType::ArcaneShot,
        AbilityType::ConcussiveShot,
        AbilityType::SerpentSting,
    ] {
        assert!(
            defs.get(&a).unwrap().projectile_visuals.is_none(),
            "{a:?} still carries projectile_visuals"
        );
    }
}

/// Aimed, Arcane and Concussive are one landing in the client, whatever our
/// RON schools say (Physical, Arcane, Physical). Serpent Sting has its own.
/// Neither is any school's stock row.
#[test]
fn the_three_shots_share_one_landing_and_the_sting_has_its_own() {
    let defs = AbilityDefinitions::default();
    let style = |a: AbilityType| landing_style(a, defs.get(&a).unwrap().spell_school);
    let aimed = style(AbilityType::AimedShot);
    assert_eq!(style(AbilityType::ArcaneShot), aimed);
    assert_eq!(style(AbilityType::ConcussiveShot), aimed);
    let sting = style(AbilityType::SerpentSting);
    assert_ne!(sting, aimed);
    for school in [
        SpellSchool::Physical,
        SpellSchool::Arcane,
        SpellSchool::Nature,
    ] {
        assert_ne!(aimed, impact_style(school));
        assert_ne!(sting, impact_style(school));
    }
    // A pure emitter landing: the client draws no flash, ring, spray or blot.
    for s in [aimed, sting] {
        assert!(s.flash.is_none() && s.ring.is_none() && s.spray.is_none() && s.blot.is_none());
    }
}

/// The shared landing is cut to smoke, stars and rune — named, as a set — and
/// plays at 0.8 size with 0.6 spread over the client's values.
#[test]
fn the_shared_landing_keeps_only_smoke_stars_and_rune() {
    let emitters = HunterShotKind::Shared.landing_emitters();
    let names: BTreeSet<&str> = emitters.iter().map(|e| e.name).collect();
    assert_eq!(
        names,
        BTreeSet::from([
            "magic_impact_chest/e3 smoke",
            "magic_impact_chest/e4 stars",
            "magic_impact_chest/e5 rune",
        ])
    );
    let smoke = emitters.iter().find(|e| e.name.ends_with("smoke")).unwrap();
    // Client 0.4 -> 0.83 puff, 1.636 yd/s.
    assert!((smoke.size[0] - 0.4 * 0.8).abs() < 1e-5);
    assert!((smoke.speed - 1.636 * 0.8 * 0.6).abs() < 1e-5);
    // Duration x1.0: the client's windows.
    assert_eq!(smoke.window, Some((0.0, 0.2)));
    let life = landing_style(AbilityType::AimedShot, SpellSchool::Physical).life();
    assert!(
        (1.0..1.4).contains(&life),
        "the shared landing is a compact burst, over by ~1.2s: {life}"
    );
}

/// Serpent Sting's cloud is the client's: a 3 s window, alpha-blended smoke
/// that can darken, and a 167 ms spray.
#[test]
fn the_serpent_landing_is_the_clients_and_lingers() {
    let emitters = HunterShotKind::SerpentSting.landing_emitters();
    let by = |suffix: &str| emitters.iter().find(|e| e.name.ends_with(suffix)).unwrap();
    assert_eq!(by("smoke").window, Some((0.0, 3.0)));
    assert!(!by("smoke").additive && by("smoke").rise);
    assert_eq!(by("spray").window, Some((0.0, 0.167)));
    assert!(!by("spray").additive);
    assert!(by("glints").additive);
    assert!((by("smoke").size[2] - 0.694).abs() < 1e-5, "client size");
    let life = landing_style(AbilityType::SerpentSting, SpellSchool::Nature).life();
    assert!(
        (life - 5.5).abs() < 1e-4,
        "3.0s window + 2.5s smoke life: {life}"
    );
}

// ── the landings in world space ────────────────────────────────────────────

/// The shared landing throws its particles around the chest and not beyond a
/// compact radius, draws no flash, and is gone with every part by its end.
#[test]
fn the_shared_landing_is_a_compact_burst_on_the_chest() {
    let mut app = app();
    let victim = spawn_victim(&mut app, Vec3::ZERO);
    land(&mut app, AbilityType::AimedShot, victim);
    for _ in 0..12 {
        app.update();
    }
    let ps = particles(&mut app);
    assert!(ps.len() >= 8, "a burst, got {} particles", ps.len());
    assert!(ps.iter().all(|p| p.3), "landing particles ride the rig");
    let chest = impact_origin(ImpactAnchor::Chest, Vec3::ZERO, false);
    let reach = ps
        .iter()
        .map(|p| (p.0 - chest).length())
        .fold(0.0_f32, f32::max);
    assert!(
        reach < 1.0,
        "a compact burst, not a firework: a particle is {reach:.2} yd out"
    );
    assert_eq!(
        count::<ImpactSprite>(&mut app),
        0,
        "the client draws no flash"
    );

    let life = landing_style(AbilityType::AimedShot, SpellSchool::Physical).life();
    for _ in 0..((life / DT) as u32 + 4) {
        app.update();
    }
    assert_eq!(count::<SchoolImpact>(&mut app), 0);
    assert_eq!(
        count::<ClientParticle>(&mut app),
        0,
        "parts outlived the rig"
    );
}

/// Serpent Sting's cloud keeps emitting well after the hit, its smoke RISES,
/// and it rides a victim that runs.
#[test]
fn the_sting_cloud_lingers_rises_and_rides_the_victim() {
    let mut app = app();
    let victim = spawn_victim(&mut app, Vec3::ZERO);
    land(&mut app, AbilityType::SerpentSting, victim);
    // Past the 167 ms spray's last particle, so only the lingering layers count.
    for _ in 0..60 {
        app.update();
    }
    let early = particles(&mut app).len();
    // Run the victim 4 yd along +Z over the next second.
    for i in 0..62 {
        if let Some(mut t) = app.world_mut().get_mut::<Transform>(victim) {
            t.translation.z = 4.0 * (i as f32 + 1.0) / 62.0;
        }
        app.update();
    }
    let ps = particles(&mut app);
    assert!(
        ps.len() > early,
        "the cloud should still be building at 2s: {early} -> {}",
        ps.len()
    );
    let smoke: Vec<_> = ps.iter().filter(|p| p.2 == 0.0 && p.1.y > 0.2).collect();
    assert!(!smoke.is_empty(), "no rising smoke");
    let c = mean(&ps.iter().map(|p| p.0).collect::<Vec<_>>());
    assert!(
        c.z > 3.0,
        "the cloud stayed behind a victim that moved to z=4: centroid {c:?}"
    );
}

/// The rune lies flat: whatever the rig's yaw, its face turns to world up.
#[test]
fn the_rune_lies_flat_under_any_yaw() {
    let rune = ClientParticle {
        age: 0.3,
        life: 0.4,
        velocity: Vec3::ZERO,
        gravity: 0.0,
        size: [0.2, 0.9, 1.7],
        mid: 0.5,
        palette: std::sync::Arc::from(Vec::<Handle<StandardMaterial>>::new()),
        step: 0,
        facing: ParticleFacing::Flat,
        owned: true,
    };
    for yaw in [0.0_f32, 0.7, 2.4, -1.9] {
        let parent = Quat::from_rotation_y(yaw);
        let world = parent * particle_rotation(&rune, parent, Quat::IDENTITY);
        let normal = world * Vec3::Z;
        assert!(normal.dot(Vec3::Y) > 0.999, "yaw {yaw}: face {normal:?}");
    }
}

// ── the missiles ───────────────────────────────────────────────────────────

fn spawn_shot(app: &mut App, ability: AbilityType) -> Entity {
    let caster = spawn_victim(app, Vec3::new(-50.0, 0.0, 0.0));
    let target = spawn_victim(app, Vec3::new(50.0, 0.0, 0.0));
    app.world_mut()
        .spawn((
            Projectile {
                caster,
                target,
                ability,
                speed: 45.0,
                caster_team: 1,
                caster_slot: 0,
                caster_class: CharacterClass::Hunter,
                caster_pet_type: None,
            },
            Transform::from_translation(Vec3::new(0.0, 1.0, 0.0))
                .with_rotation(Quat::from_rotation_arc(Vec3::Z, Vec3::X)),
        ))
        .id()
}

/// Fly the missile along +X at 45 yd/s for `frames` frames.
fn fly(app: &mut App, shot: Entity, frames: u32) {
    for _ in 0..frames {
        if let Some(mut t) = app.world_mut().get_mut::<Transform>(shot) {
            t.translation.x += 45.0 * DT;
        }
        app.update();
    }
}

/// The core is 0.6 yd across; the particles are left BEHIND the missile in
/// world space, with a continuous ribbon.
#[test]
fn the_shot_sheds_its_particles_and_ribbon_behind_it() {
    let mut app = app();
    let shot = spawn_shot(&mut app, AbilityType::ArcaneShot);
    fly(&mut app, shot, 30);

    let mut cores = app
        .world_mut()
        .query_filtered::<&GlobalTransform, With<HunterShotCore>>();
    let widths: Vec<f32> = cores.iter(app.world()).map(|g| g.scale().x).collect();
    assert_eq!(widths.len(), 2, "a glow and its white-hot centre");
    let widest = widths.iter().copied().fold(0.0_f32, f32::max);
    assert!((widest - HUNTER_SHOT_CORE_DIAMETER).abs() < 1e-3);

    let x = app.world().get::<Transform>(shot).unwrap().translation.x;
    let ps = particles(&mut app);
    assert!(ps.len() > 30, "100/s stream plus sparks, got {}", ps.len());
    assert!(ps.iter().all(|p| !p.3), "missile particles are world-space");
    assert!(
        ps.iter().all(|p| p.0.x <= x + 0.8),
        "a particle is ahead of the missile"
    );
    // The stream spreads back along the path, not clumped at the head.
    let behind = ps.iter().filter(|p| p.0.x < x - 5.0).count();
    assert!(
        behind > 5,
        "only {behind} particles trail more than 5yd back"
    );

    let mut trails = app.world_mut().query::<(&BoltTrail, &Transform)>();
    let segs: Vec<f32> = trails
        .iter(app.world())
        .map(|(_, t)| t.translation.x)
        .collect();
    assert!(!segs.is_empty(), "no ribbon");
    assert!(
        segs.iter().all(|sx| *sx <= x),
        "ribbon ahead of the missile"
    );

    // The projectile resolves: the core goes with it, the shed particles stay
    // and fade on their own clock.
    app.world_mut().entity_mut(shot).despawn();
    app.update();
    assert_eq!(count::<HunterShotCore>(&mut app), 0);
    assert!(!particles(&mut app).is_empty());
    for _ in 0..80 {
        app.update();
    }
    assert_eq!(count::<ClientParticle>(&mut app), 0);
}

/// Serpent Sting's sparks drip: they end up below the flight path, where the
/// shared shot's hang level with it.
#[test]
fn serpent_sparks_drip_and_the_shared_shots_do_not() {
    let sag = |ability: AbilityType| {
        let mut app = app();
        let shot = spawn_shot(&mut app, ability);
        fly(&mut app, shot, 45);
        // The sting's two spark emitters are its only ones with gravity; the
        // shared shot has none, so every one of its particles is measured.
        let ys: Vec<f32> = particles(&mut app)
            .iter()
            .filter(|p| p.2 > 0.0 || ability != AbilityType::SerpentSting)
            .map(|p| p.0.y)
            .collect();
        assert!(ys.len() > 10);
        1.0 - ys.iter().sum::<f32>() / ys.len() as f32
    };
    let sting = sag(AbilityType::SerpentSting);
    let shared = sag(AbilityType::AimedShot);
    assert!(
        sting > 0.06 && sting > shared + 0.05,
        "sting sparks sagged {sting:.3} yd against the shared shot's {shared:.3}"
    );
    assert!(shared.abs() < 0.05, "shared shot sagged {shared:.3} yd");
}

/// Nothing a shot draws casts a shadow — a glow does not.
#[test]
fn no_part_of_a_shot_casts_a_shadow() {
    let mut app = app();
    let shot = spawn_shot(&mut app, AbilityType::SerpentSting);
    fly(&mut app, shot, 10);
    let victim = spawn_victim(&mut app, Vec3::Z * 5.0);
    land(&mut app, AbilityType::SerpentSting, victim);
    land(&mut app, AbilityType::AimedShot, victim);
    for _ in 0..10 {
        app.update();
    }
    let mut q = app
        .world_mut()
        .query_filtered::<(Entity, Option<&NotShadowCaster>), With<Mesh3d>>();
    let mut n = 0;
    for (e, flag) in q.iter(app.world()) {
        n += 1;
        assert!(flag.is_some(), "{e:?} casts a shadow");
    }
    assert!(n > 30, "expected many pieces, saw {n}");
}

// ── the Auto Shot arrow ────────────────────────────────────────────────────

fn loose_arrow(app: &mut App, from: Vec3, target: Entity) -> Entity {
    let aim = {
        let t = app.world().get::<Transform>(target).unwrap().translation;
        impact_origin(ImpactAnchor::Chest, t, false)
    };
    app.world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  mut meshes: ResMut<Assets<Mesh>>,
                  mut materials: ResMut<Assets<StandardMaterial>>,
                  mut images: ResMut<Assets<Image>>| {
                let assets = AutoShotArrowAssets::build(&mut meshes, &mut materials, &mut images);
                spawn_auto_shot_arrow(&mut commands, &assets, from, target, aim);
            },
        )
        .unwrap();
    let mut q = app
        .world_mut()
        .query_filtered::<Entity, With<CosmeticArrow>>();
    q.iter(app.world()).next().expect("an arrow")
}

/// The arrow is the client's `arrowflight_01` proportions: 1.52 yd from nock to
/// tip, 0.14 yd across, with the tip at the entity's origin and the shaft
/// hanging back along -Z.
#[test]
fn the_arrow_has_the_clients_proportions() {
    let mut app = app();
    let target = spawn_victim(&mut app, Vec3::new(0.0, 0.0, 30.0));
    let arrow = loose_arrow(&mut app, Vec3::ZERO, target);
    let children: Vec<Entity> = app.world().get::<Children>(arrow).unwrap().iter().collect();
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for child in children {
        let pose = *app.world().get::<Transform>(child).unwrap();
        let handle = app.world().get::<Mesh3d>(child).unwrap().0.clone();
        let meshes = app.world().resource::<Assets<Mesh>>();
        let aabb = meshes.get(&handle).unwrap().compute_aabb().unwrap();
        let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                for sz in [-1.0, 1.0] {
                    let p = pose.transform_point(c + h * Vec3::new(sx, sy, sz));
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
        }
    }
    assert!(
        hi.z.abs() < 1e-3,
        "the tip must sit at the origin, got {}",
        hi.z
    );
    assert!(
        (lo.z + AUTO_ARROW_LENGTH).abs() < 1e-3,
        "nock at {} — the arrow is 1.52 yd",
        lo.z
    );
    let across = (hi.x - lo.x).max(hi.y - lo.y);
    assert!((across - AUTO_ARROW_WIDTH).abs() < 1e-3, "{across} across");
}

/// The arrow homes on a target that RUNS and ends at its chest — not at the
/// point the target stood when the shot landed — then is gone.
#[test]
fn the_arrow_ends_at_a_running_target() {
    let mut app = app();
    let target = spawn_victim(&mut app, Vec3::new(0.0, 0.0, 30.0));
    let arrow = loose_arrow(&mut app, Vec3::Y * 1.1, target);
    let mut last = Vec3::ZERO;
    let mut frames = 0;
    while app.world().get_entity(arrow).is_ok() {
        // The victim sprints sideways at 7 yd/s.
        app.world_mut()
            .get_mut::<Transform>(target)
            .unwrap()
            .translation
            .x += 7.0 * DT;
        app.update();
        if let Some(t) = app.world().get::<Transform>(arrow) {
            last = t.translation;
        }
        frames += 1;
        assert!(frames < 200, "the arrow never arrived");
    }
    let now = app.world().get::<Transform>(target).unwrap().translation;
    let chest = impact_origin(ImpactAnchor::Chest, now, false);
    assert!(
        (last - chest).length() < AUTO_ARROW_SPEED * DT + 0.2,
        "the arrow's last pose {last:?} is not at the victim's chest {chest:?}"
    );
    assert!(
        now.x > 3.0 && (last.x - 0.0).abs() > 2.0,
        "the victim ran {} yd and the arrow followed to x={}",
        now.x,
        last.x
    );
    assert!(
        count::<BoltTrail>(&mut app) > 0,
        "the arrow lays its ribbon"
    );
}

/// A victim that dies mid-flight leaves the arrow its last aim point; it still
/// arrives and retires.
#[test]
fn an_arrow_survives_losing_its_victim() {
    let mut app = app();
    let target = spawn_victim(&mut app, Vec3::new(0.0, 0.0, 20.0));
    let arrow = loose_arrow(&mut app, Vec3::ZERO, target);
    app.update();
    app.world_mut().despawn(target);
    for _ in 0..60 {
        app.update();
    }
    assert!(app.world().get_entity(arrow).is_err());
}
