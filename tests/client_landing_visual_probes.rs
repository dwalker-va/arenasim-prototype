//! Probes for the client landings (`rendering/effects/client_landings.rs`):
//! the four interrupt landings, Heroic Strike's empowered swing, Mind Blast's
//! head smoulder, Holy Shock's damage landing — and the Web disc
//! (`web_missile.rs`).
//!
//! These assert WORLD-SPACE geometry and the routing contract, not the fields
//! the rigs store: that an interrupt's mark sits on its victim's chest and a
//! fizzle leaves none, that the shockwave grows and the seal closes, that the
//! smoulder rises off the head, that Holy Shock's stars gather before its
//! sparks fall, that only the EMPOWERED swing is dressed, and that the web
//! faces its flight and spins about it. The sim side is pinned too: the
//! interrupt and the empowered swing are named on the markers the sim spawns.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window, no
//! GPU. `TransformPlugin` is load-bearing: without it `GlobalTransform` never
//! propagates and every assertion below would read a child's LOCAL pose.

use std::f32::consts::TAU;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::abilities::{AbilityType, SpellSchool};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::combat_core::{combat_auto_attack, process_interrupts};
use arenasim::states::play_match::components::{
    AutoAttackKind, AutoAttackSwing, CastEnding, CastEndingKind, CastingState, ClientParticle,
    Combatant, GameRng, HeroicStrikeSwing, ImpactAnchor, InterruptPending, InterruptedBy,
    MatchCountdown, ParticleFacing, Projectile, SchoolImpact,
};
use arenasim::states::play_match::map_config::ActiveMapGeometry;
use arenasim::states::play_match::{
    animate_client_particles, animate_school_impacts, impact_origin, landing_style,
    spawn_heroic_strike_flourish, spawn_interrupt_landings, spawn_projectile_visuals,
    spawn_school_impacts, spawn_web_missile_visuals, spin_web_discs, InterruptLanding, WeaponTrail,
    WebDisc, WEB_DISC_DIAMETER, WEB_DISC_SPIN_REV_PER_SEC,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;

const INTERRUPTS: [AbilityType; 4] = [
    AbilityType::Kick,
    AbilityType::Pummel,
    AbilityType::SpellLock,
    AbilityType::WindShear,
];

/// The sim's marker entities are despawned by their last graphical consumer
/// (`consume_cast_ending_signals`, `consume_swing_signals`) on the tick they
/// are read. This stands in for both, so a marker is read exactly once.
fn retire_markers(
    mut commands: Commands,
    markers: Query<Entity, Or<(With<CastEnding>, With<AutoAttackSwing>)>>,
) {
    for e in markers.iter() {
        commands.entity(e).despawn();
    }
}

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
    app.insert_resource(AbilityDefinitions::default());
    app.add_systems(
        Update,
        (
            spawn_interrupt_landings,
            spawn_heroic_strike_flourish,
            retire_markers,
            spawn_school_impacts,
            animate_school_impacts,
            animate_client_particles,
        )
            .chain(),
    );
    app
}

fn tick(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.update();
    }
}

fn spawn_unit(app: &mut App, class: CharacterClass, at: Vec3) -> Entity {
    app.world_mut()
        .spawn((Combatant::new(0, 0, class), Transform::from_translation(at)))
        .id()
}

/// Every live client particle: world position, world scale, facing, age.
fn particles(app: &mut App) -> Vec<(Vec3, f32, ParticleFacing, f32, Vec3)> {
    let mut q = app
        .world_mut()
        .query::<(&ClientParticle, &GlobalTransform)>();
    q.iter(app.world())
        .filter(|(p, _)| p.age < p.life)
        .map(|(p, g)| {
            let (scale, _, at) = g.to_scale_rotation_translation();
            (at, scale.x, p.facing, p.age, p.velocity)
        })
        .collect()
}

fn count<T: Component>(app: &mut App) -> usize {
    let mut q = app.world_mut().query_filtered::<Entity, With<T>>();
    q.iter(app.world()).count()
}

fn mean(points: &[Vec3]) -> Vec3 {
    points.iter().copied().sum::<Vec3>() / points.len().max(1) as f32
}

fn chest(at: Vec3) -> Vec3 {
    impact_origin(ImpactAnchor::Chest, at, false)
}

/// Queue an interrupt's `CastEnding` exactly as `process_interrupts` spawns it.
fn interrupt(app: &mut App, ability: AbilityType, victim: Entity, interrupter: Entity) {
    app.world_mut().spawn((
        CastEnding {
            caster: victim,
            kind: CastEndingKind::Interrupted,
        },
        InterruptedBy {
            ability,
            interrupter,
        },
    ));
}

// ── the interrupts ─────────────────────────────────────────────────────────

/// Each interrupt lands its mark on the chest of the unit whose cast it cut.
/// A fizzle, and a cast broken by crowd control (an `Interrupted` ending that
/// no interrupt ABILITY caused), land nothing — those are the two endings an
/// interrupt must now read apart from.
#[test]
fn an_interrupt_marks_its_victims_chest_and_a_fizzle_marks_nothing() {
    for ability in INTERRUPTS {
        let mut app = app();
        let victim = spawn_unit(&mut app, CharacterClass::Mage, Vec3::ZERO);
        let interrupter = spawn_unit(&mut app, CharacterClass::Rogue, Vec3::new(3.0, 0.0, 0.0));
        let fizzler = spawn_unit(&mut app, CharacterClass::Priest, Vec3::new(0.0, 0.0, 8.0));
        let cc_broken = spawn_unit(&mut app, CharacterClass::Warlock, Vec3::new(0.0, 0.0, -8.0));
        interrupt(&mut app, ability, victim, interrupter);
        app.world_mut().spawn(CastEnding {
            caster: fizzler,
            kind: CastEndingKind::Fizzled,
        });
        app.world_mut().spawn(CastEnding {
            caster: cc_broken,
            kind: CastEndingKind::Interrupted,
        });
        tick(&mut app, 8);

        assert_eq!(count::<SchoolImpact>(&mut app), 1, "{ability:?}");
        let ps = particles(&mut app);
        assert!(!ps.is_empty(), "{ability:?} landed nothing on its victim");
        let target = chest(Vec3::ZERO);
        for (at, ..) in &ps {
            assert!(
                at.distance(target) < 1.0,
                "{ability:?}: a particle at {at:?} is off the victim's chest {target:?}"
            );
        }
    }
}

/// The mark rides a victim who keeps moving.
#[test]
fn the_mark_rides_a_moving_victim() {
    let mut app = app();
    let victim = spawn_unit(&mut app, CharacterClass::Mage, Vec3::ZERO);
    let interrupter = spawn_unit(&mut app, CharacterClass::Rogue, Vec3::new(3.0, 0.0, 0.0));
    interrupt(&mut app, AbilityType::SpellLock, victim, interrupter);
    tick(&mut app, 4);
    app.world_mut()
        .get_mut::<Transform>(victim)
        .unwrap()
        .translation = Vec3::new(0.0, 0.0, 5.0);
    tick(&mut app, 10);
    let c = mean(&particles(&mut app).iter().map(|p| p.0).collect::<Vec<_>>());
    assert!(
        c.distance(chest(Vec3::new(0.0, 0.0, 5.0))) < 0.8,
        "the seal stayed behind: centroid {c:?}"
    );
}

/// Kick's shockwave GROWS as it fades; Spell Lock's runes SHRINK onto the
/// chest. Measured on one particle's world scale across its life.
#[test]
fn the_shockwave_grows_and_the_seal_closes() {
    // (ability, facing of the particle to follow, frames until the first one
    // is born, expect growth). Kick's first ring is due at 0.1s (10/s), Spell
    // Lock's first rune at about 0.2s (5.1/s and 6/s).
    for (ability, facing, born_by, grows) in [
        (AbilityType::Kick, ParticleFacing::Camera, 8, true),
        (AbilityType::SpellLock, ParticleFacing::Seal, 15, false),
    ] {
        let mut app = app();
        let victim = spawn_unit(&mut app, CharacterClass::Mage, Vec3::ZERO);
        let interrupter = spawn_unit(&mut app, CharacterClass::Rogue, Vec3::new(3.0, 0.0, 0.0));
        interrupt(&mut app, ability, victim, interrupter);
        tick(&mut app, born_by);
        let first = {
            let mut q = app.world_mut().query::<(Entity, &ClientParticle)>();
            q.iter(app.world())
                .filter(|(_, p)| p.facing == facing)
                .min_by(|a, b| b.1.age.partial_cmp(&a.1.age).unwrap())
                .map(|(e, _)| e)
                .expect("no particle of that facing")
        };
        let scale = |app: &mut App| {
            app.world()
                .get::<GlobalTransform>(first)
                .unwrap()
                .to_scale_rotation_translation()
                .0
                .x
        };
        let early = scale(&mut app);
        // Kick's ring lives 0.3s; Spell Lock's runes about 1.2s.
        tick(&mut app, if grows { 14 } else { 60 });
        let late = scale(&mut app);
        if grows {
            assert!(late > early * 2.0, "{ability:?}: {early} -> {late}");
        } else {
            assert!(late < early * 0.6, "{ability:?}: {early} -> {late}");
        }
    }
}

/// Wind Shear is the Kick ring, blue. Read off the material the particle
/// actually wears.
#[test]
fn wind_shear_rings_in_wind_blue_where_kick_rings_white() {
    let tint = |ability: AbilityType| {
        let mut app = app();
        let victim = spawn_unit(&mut app, CharacterClass::Mage, Vec3::ZERO);
        let interrupter = spawn_unit(&mut app, CharacterClass::Shaman, Vec3::new(3.0, 0.0, 0.0));
        interrupt(&mut app, ability, victim, interrupter);
        tick(&mut app, 8);
        let handle = {
            let mut q = app
                .world_mut()
                .query_filtered::<&MeshMaterial3d<StandardMaterial>, With<ClientParticle>>();
            q.iter(app.world()).next().expect("a ring").0.clone()
        };
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        materials.get(&handle).unwrap().base_color.to_srgba()
    };
    let (kick, wind) = (tint(AbilityType::Kick), tint(AbilityType::WindShear));
    assert!(
        kick.blue - kick.red < 0.05,
        "Kick's ring should be near-white: {kick:?}"
    );
    assert!(
        wind.blue - wind.red > 0.2,
        "Wind Shear's ring should be wind-blue: {wind:?}"
    );
}

// ── the sim names the interrupt ────────────────────────────────────────────

/// `process_interrupts` names the interrupt on the ending it spawns — the one
/// fact the landing needs from the sim.
#[test]
fn process_interrupts_names_the_interrupt_on_its_ending() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(CombatLog::default())
        .insert_resource(AbilityDefinitions::default())
        .add_systems(Update, process_interrupts);
    let kicker = app
        .world_mut()
        .spawn(Combatant::new(1, 0, CharacterClass::Rogue))
        .id();
    let caster = app
        .world_mut()
        .spawn(Combatant::new(2, 0, CharacterClass::Mage))
        .id();
    app.world_mut().entity_mut(caster).insert(CastingState::new(
        AbilityType::Frostbolt,
        kicker,
        2.5,
    ));
    app.world_mut().spawn(InterruptPending {
        caster: kicker,
        target: caster,
        ability: AbilityType::Kick,
        lockout_duration: 5.0,
    });
    app.update();
    let mut q = app.world_mut().query::<(&CastEnding, &InterruptedBy)>();
    let named: Vec<_> = q
        .iter(app.world())
        .map(|(e, by)| (e.caster, e.kind, by.ability, by.interrupter))
        .collect();
    assert_eq!(
        named,
        vec![(
            caster,
            CastEndingKind::Interrupted,
            AbilityType::Kick,
            kicker
        )]
    );
}

// ── Heroic Strike ──────────────────────────────────────────────────────────

fn swing(app: &mut App, attacker: Entity, target: Entity, heroic: bool) {
    let mut e = app.world_mut().spawn(AutoAttackSwing {
        attacker,
        target,
        kind: AutoAttackKind::Melee,
        is_crit: false,
    });
    if heroic {
        e.insert(HeroicStrikeSwing);
    }
}

/// Only the EMPOWERED swing is dressed: an ordinary one lays no trail and
/// lands nothing.
#[test]
fn only_the_empowered_swing_is_dressed() {
    let mut app = app();
    let warrior = spawn_unit(&mut app, CharacterClass::Warrior, Vec3::new(-2.0, 0.0, 0.0));
    let victim = spawn_unit(&mut app, CharacterClass::Mage, Vec3::ZERO);
    swing(&mut app, warrior, victim, false);
    tick(&mut app, 20);
    assert_eq!(count::<WeaponTrail>(&mut app), 0);
    assert_eq!(count::<SchoolImpact>(&mut app), 0);
    assert!(particles(&mut app).is_empty());

    swing(&mut app, warrior, victim, true);
    tick(&mut app, 1);
    assert_eq!(
        count::<WeaponTrail>(&mut app),
        1,
        "the empowered swing lays a trail"
    );
    assert_eq!(count::<SchoolImpact>(&mut app), 1, "and lands a mark");
}

/// The landing waits for the blade: nothing shows while the weapon is still
/// wound up, then the ring opens on the victim's chest and grows past the
/// body.
#[test]
fn the_heroic_strike_ring_lands_when_the_blade_does() {
    let mut app = app();
    let warrior = spawn_unit(&mut app, CharacterClass::Warrior, Vec3::new(-2.0, 0.0, 0.0));
    let victim = spawn_unit(&mut app, CharacterClass::Mage, Vec3::ZERO);
    swing(&mut app, warrior, victim, true);
    // The ordinary auto's release reaches the victim 0.12s in.
    tick(&mut app, 5);
    assert!(
        particles(&mut app).is_empty(),
        "the landing fired while the blade was still travelling"
    );
    // (The first update of a MinimalPlugins app advances no time.)
    tick(&mut app, 8);
    let ps = particles(&mut app);
    let rings: Vec<_> = ps
        .iter()
        .filter(|p| p.2 == ParticleFacing::Camera && p.4 == Vec3::ZERO)
        .collect();
    assert!(!ps.is_empty(), "no landing after the blade arrived");
    assert!(!rings.is_empty(), "no ring: {ps:?}");
    for (at, ..) in &ps {
        assert!(at.distance(chest(Vec3::ZERO)) < 1.0, "{at:?}");
    }
    tick(&mut app, 20);
    let widest = particles(&mut app)
        .iter()
        .filter(|p| p.4 == Vec3::ZERO)
        .map(|p| p.1)
        .fold(0.0_f32, f32::max);
    assert!(widest > 1.0, "the ring should open past the body: {widest}");
}

/// The sim marks the swing that carries the queued bonus, and only that one:
/// the bonus is consumed by it, so the next swing is ordinary again.
#[test]
fn the_sim_marks_only_the_swing_that_carries_the_bonus() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(TICK))
        .insert_resource(MatchCountdown {
            time_remaining: 0.0,
            gates_opened: true,
        })
        .insert_resource(CombatLog::default())
        .insert_resource(GameRng::from_seed(7))
        .insert_resource(AbilityDefinitions::default())
        .insert_resource(ActiveMapGeometry {
            bounds: Default::default(),
            volumes: Vec::new(),
            cover_anchors: Vec::new(),
        })
        .add_systems(Update, combat_auto_attack);
    let target = app
        .world_mut()
        .spawn((
            Combatant::new(2, 0, CharacterClass::Mage),
            Transform::from_translation(Vec3::new(1.5, 0.0, 0.0)),
        ))
        .id();
    let mut warrior = Combatant::new(1, 0, CharacterClass::Warrior);
    warrior.attack_speed = 5.0;
    warrior.target = Some(target);
    warrior.next_attack_bonus_damage = warrior.attack_damage * 0.5;
    let warrior = app
        .world_mut()
        .spawn((warrior, Transform::from_translation(Vec3::ZERO)))
        .id();

    let mut marks = Vec::new();
    for _ in 0..120 {
        app.update();
        let mut q = app
            .world_mut()
            .query::<(Entity, &AutoAttackSwing, Option<&HeroicStrikeSwing>)>();
        let seen: Vec<_> = q
            .iter(app.world())
            .filter(|(_, s, _)| s.attacker == warrior)
            .map(|(e, _, h)| (e, h.is_some()))
            .collect();
        for (e, heroic) in seen {
            marks.push(heroic);
            app.world_mut().despawn(e);
        }
    }
    assert!(marks.len() >= 3, "too few swings to judge: {marks:?}");
    assert!(marks[0], "the first swing carried the bonus");
    assert!(
        marks[1..].iter().all(|h| !h),
        "only the first swing is empowered: {marks:?}"
    );
}

// ── Mind Blast ─────────────────────────────────────────────────────────────

fn land(app: &mut App, ability: AbilityType, victim: Entity, anchor: ImpactAnchor) {
    let school = AbilityDefinitions::default()
        .get(&ability)
        .unwrap()
        .spell_school;
    app.world_mut().spawn(SchoolImpact {
        target: victim,
        ability,
        school,
        anchor,
        from: Vec3::X,
        magnitude: 0.1,
        is_crit: false,
        age: 0.0,
    });
}

/// Mind Blast smoulders ON THE HEAD: it keeps emitting for over a second, its
/// flames and smoke rise clear of the crown, and it is over in a few seconds.
#[test]
fn mind_blast_smoulders_on_the_head_and_rises() {
    let mut app = app();
    let victim = spawn_unit(&mut app, CharacterClass::Priest, Vec3::ZERO);
    land(&mut app, AbilityType::MindBlast, victim, ImpactAnchor::Head);
    let head = impact_origin(ImpactAnchor::Head, Vec3::ZERO, false);

    tick(&mut app, 30); // 0.48s
    let early = particles(&mut app);
    assert!(
        early.len() > 10,
        "the smoulder has barely started: {}",
        early.len()
    );
    tick(&mut app, 45); // 1.2s — still inside the flames' 200-1400 ms hold
    let mid = particles(&mut app);
    assert!(
        mid.len() > early.len(),
        "a smoulder keeps emitting: {} at 0.48s, {} at 1.2s",
        early.len(),
        mid.len()
    );
    let c = mean(&mid.iter().map(|p| p.0).collect::<Vec<_>>());
    assert!(
        c.y > head.y + 0.1,
        "the smoulder should rise off the head: {c:?}"
    );
    assert!(
        (Vec3::new(c.x, 0.0, c.z)).length() < 0.4,
        "and stay over it: {c:?}"
    );
    // The chest is not where it plays.
    assert!(
        mid.iter().all(|p| p.0.y > chest(Vec3::ZERO).y + 0.2),
        "a smoulder particle sank to the chest"
    );

    // Over by the flames' 2s sequence plus the smoke's 2.25s life.
    let life = landing_style(AbilityType::MindBlast, SpellSchool::Shadow).life();
    assert!(life > 2.0 && life < 5.0, "{life}");
    tick(&mut app, ((life + 0.2) / DT) as u32);
    assert_eq!(
        count::<SchoolImpact>(&mut app),
        0,
        "the rig outlived its life"
    );
    assert!(particles(&mut app).is_empty());
}

// ── Holy Shock ─────────────────────────────────────────────────────────────

/// Holy Shock's damage landing GATHERS before it bursts: in the first 133 ms
/// only the converging motes play, every one travelling inward; then the
/// sparks arc out and fall; and it is all over in under two seconds.
#[test]
fn holy_shock_gathers_then_bursts_and_falls() {
    let mut app = app();
    let victim = spawn_unit(&mut app, CharacterClass::Warlock, Vec3::ZERO);
    land(
        &mut app,
        AbilityType::HolyShock,
        victim,
        ImpactAnchor::Chest,
    );
    let centre = chest(Vec3::ZERO);

    tick(&mut app, 6); // 0.1s
    let gathering = particles(&mut app);
    assert!(!gathering.is_empty(), "nothing gathers");
    for (at, _, _, _, v) in &gathering {
        assert!(
            v.dot(*at - centre) < 0.0,
            "a mote at {at:?} is not travelling in (v {v:?})"
        );
    }

    // By 0.45s the burst is up: particles thrown well clear of the chest.
    tick(&mut app, 22);
    let burst = particles(&mut app);
    assert!(
        burst.len() > gathering.len() * 2,
        "{} -> {}",
        gathering.len(),
        burst.len()
    );
    // Measured on the thrown sparks (the gathering motes are BORN out there).
    let reach = {
        let mut q = app
            .world_mut()
            .query::<(&ClientParticle, &GlobalTransform)>();
        q.iter(app.world())
            .filter(|(p, _)| p.gravity > 0.0 && p.age < p.life)
            .map(|(_, g)| g.translation().distance(centre))
            .fold(0.0_f32, f32::max)
    };
    assert!(reach > 0.3, "the sparks stayed inside the chest: {reach}");

    // The sparks fall: every young spark's WORLD height bends downward — its
    // second difference over two equal spans is negative, whichever way it
    // was thrown.
    let young_sparks = {
        let mut q = app.world_mut().query::<(Entity, &ClientParticle)>();
        q.iter(app.world())
            .filter(|(_, p)| p.gravity > 0.0 && p.age < 0.3)
            .map(|(e, _)| e)
            .collect::<Vec<_>>()
    };
    assert!(
        young_sparks.len() >= 5,
        "too few sparks: {}",
        young_sparks.len()
    );
    let heights = |app: &mut App| -> Vec<f32> {
        young_sparks
            .iter()
            .map(|e| {
                app.world()
                    .get::<GlobalTransform>(*e)
                    .unwrap()
                    .translation()
                    .y
            })
            .collect()
    };
    let y0 = heights(&mut app);
    tick(&mut app, 8);
    let y1 = heights(&mut app);
    tick(&mut app, 8);
    let y2 = heights(&mut app);
    for i in 0..young_sparks.len() {
        let bend = y2[i] - 2.0 * y1[i] + y0[i];
        assert!(
            bend < -0.01,
            "spark {i} does not fall: {} {} {}",
            y0[i],
            y1[i],
            y2[i]
        );
    }

    let life = landing_style(AbilityType::HolyShock, SpellSchool::Holy).life();
    assert!(life < 2.0, "{life}");
    tick(&mut app, ((life + 0.2) / DT) as u32);
    assert_eq!(count::<SchoolImpact>(&mut app), 0);
}

// ── routing ────────────────────────────────────────────────────────────────

/// The landing router answers for exactly the interrupts, and each one's
/// landing is pure client emitters — no school flash, ring or spray.
#[test]
fn every_interrupt_routes_to_an_emitter_landing() {
    let defs = AbilityDefinitions::default();
    let interrupts: Vec<AbilityType> = defs
        .iter()
        .filter(|(_, c)| c.is_interrupt)
        .map(|(a, _)| *a)
        .collect();
    let mut routed: Vec<AbilityType> = defs
        .iter()
        .map(|(a, _)| *a)
        .filter(|a| InterruptLanding::for_ability(*a).is_some())
        .collect();
    let mut want = interrupts.clone();
    routed.sort();
    want.sort();
    assert_eq!(
        routed, want,
        "the router and the config's interrupts disagree"
    );
    for a in interrupts {
        let style = landing_style(a, SpellSchool::Physical);
        assert!(!style.emitters.is_empty(), "{a:?}");
        assert!(style.flash.is_none() && style.ring.is_none() && style.spray.is_none());
    }
}

// ── the Web ────────────────────────────────────────────────────────────────

fn web_app() -> App {
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
            spawn_projectile_visuals,
            spawn_web_missile_visuals,
            spin_web_discs,
        )
            .chain(),
    );
    app
}

/// The web flies as a disc 1.2 yd across, FACING its flight and spinning
/// about it at 1.5 rev/s — and the generic projectile draws nothing for it.
#[test]
fn the_web_is_a_spinning_disc_facing_its_flight() {
    let mut app = web_app();
    let spider = spawn_unit(&mut app, CharacterClass::Hunter, Vec3::new(-10.0, 0.0, 0.0));
    let victim = spawn_unit(&mut app, CharacterClass::Mage, Vec3::new(10.0, 0.0, 0.0));
    let flight = Vec3::new(1.0, 0.0, 0.0);
    let web = app
        .world_mut()
        .spawn((
            Projectile {
                caster: spider,
                target: victim,
                ability: AbilityType::SpiderWeb,
                speed: 50.0,
                caster_team: 1,
                caster_slot: 0,
                caster_class: CharacterClass::Hunter,
                caster_pet_type: None,
            },
            Transform::from_translation(Vec3::new(-8.0, 1.0, 0.0))
                .with_rotation(Quat::from_rotation_arc(Vec3::Z, flight)),
        ))
        .id();
    tick(&mut app, 2);
    assert!(
        app.world().get::<Mesh3d>(web).is_none(),
        "the generic projectile still hangs its own mesh on the web"
    );

    let disc = |app: &mut App| {
        let mut q = app.world_mut().query::<(&WebDisc, &GlobalTransform)>();
        let (_, g) = q.iter(app.world()).next().expect("no web disc");
        g.to_scale_rotation_translation()
    };
    let (scale, rot0, at) = disc(&mut app);
    assert!((scale.x - WEB_DISC_DIAMETER).abs() < 1e-3, "{scale:?}");
    assert!(at.distance(Vec3::new(-8.0, 1.0, 0.0)) < 1e-3, "{at:?}");
    assert!(
        (rot0 * Vec3::Z).dot(flight) > 0.999,
        "the disc must face along its flight: {:?}",
        rot0 * Vec3::Z
    );

    // A quarter turn about the flight axis takes a quarter of a revolution.
    let frames = (0.25 / WEB_DISC_SPIN_REV_PER_SEC / DT).round() as u32;
    let x0 = rot0 * Vec3::X;
    tick(&mut app, frames);
    let (_, rot1, _) = disc(&mut app);
    let x1 = rot1 * Vec3::X;
    assert!(
        (rot1 * Vec3::Z).dot(flight) > 0.999,
        "it tipped off its flight"
    );
    let turned = x0.angle_between(x1);
    let expected = frames as f32 * DT * WEB_DISC_SPIN_REV_PER_SEC * TAU;
    assert!(
        (turned - expected).abs() < 0.05,
        "spun {turned:.3} rad in {frames} frames, expected {expected:.3}"
    );
}
