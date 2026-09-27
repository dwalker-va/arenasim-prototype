//! Probes for the slow treatment (`rendering/effects/slow_ring.rs`): the bind
//! ring at a slowed victim's feet and the scuff along its path, drawn only
//! while the victim moves.
//!
//! The load-bearing claim is the MOVING gate, and it is tested through the real
//! gait: `update_walk_animation` runs first and keeps the idle clock the
//! treatment reads, so "moving" here is the gait's own definition. Everything is
//! asserted in world space — where the ring is, how wide it is off its mesh,
//! where the scuff lies — never off the stored fields.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` — no window, no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::render::mesh::MeshAabb;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::{AbilityType, SpellSchool};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, Combatant, SchoolImpact, SlowBindRing, SlowScuff,
    SlowTrailEmitter, SlowZone, VisualBody, WalkAnim,
};
use arenasim::states::play_match::{
    frost_armor_movement_slow_aura, slow_zone_system, update_slow_rings, update_slow_scuffs,
    update_slow_treatment, update_walk_animation, SlowTint, SLOW_RING_BAND_WIDTH,
    SLOW_RING_DIAMETER_END, SLOW_RING_DIAMETER_START, SLOW_RING_LIFETIME, SLOW_RING_PERIOD,
    SLOW_RING_Y, SLOW_SCUFF_LIFETIME, SLOW_SCUFF_Y,
};
use arenasim::CharacterClass;

/// 20ms: fine enough to see the ring's 0.395s life in ~20 frames.
const TICK: Duration = Duration::from_millis(20);
const DT: f32 = 0.02;
/// Frostbolt's slowed walk: 5 yd/s base x 0.7.
const SLOWED_SPEED: f32 = 3.5;

fn slow(name: &str, magnitude: f32) -> Aura {
    Aura {
        effect_type: AuraType::MovementSpeedSlow,
        duration: 8.0,
        magnitude,
        break_on_damage_threshold: -1.0,
        ability_name: name.to_string(),
        ..Default::default()
    }
}

struct Harness {
    app: App,
}

impl Harness {
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
        // The gait, then the treatment, as registered in `states/mod.rs`.
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
        // Prime `Time` so the first measured frame has a real delta.
        app.update();
        Harness { app }
    }

    /// A combatant with a `VisualBody` child, mirroring the real hierarchy.
    fn spawn_unit(&mut self, at: Vec3, auras: Vec<Aura>) -> Entity {
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
                Combatant::new(1, 0, CharacterClass::Warrior),
                // Standing settled: the gait's idle clock is well past its
                // hold, so the unit counts as moving only once it walks.
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

    fn pos(&self, e: Entity) -> Vec3 {
        self.app.world().get::<Transform>(e).unwrap().translation
    }

    /// Walk `e` along `dir` at `speed` for `frames`, the way the sim moves it.
    fn walk(&mut self, e: Entity, dir: Vec3, speed: f32, frames: usize) {
        for _ in 0..frames {
            let p = self.pos(e) + dir.normalize() * speed * DT;
            self.app
                .world_mut()
                .get_mut::<Transform>(e)
                .unwrap()
                .translation = p;
            self.app.update();
        }
    }

    fn idle(&mut self, frames: usize) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    /// Every live ring stroke: (world transform, mesh outer radius, band width).
    fn rings(&mut self) -> Vec<(SlowBindRing, Transform, f32, f32)> {
        let found: Vec<(SlowBindRing, Transform, Handle<Mesh>)> = {
            let mut q = self
                .app
                .world_mut()
                .query::<(&SlowBindRing, &Transform, &Mesh3d)>();
            q.iter(self.app.world())
                .map(|(r, t, m)| (*r, *t, m.0.clone()))
                .collect()
        };
        let meshes = self.app.world().resource::<Assets<Mesh>>();
        found
            .into_iter()
            .map(|(r, t, h)| {
                let mesh = meshes.get(&h).unwrap();
                let outer = mesh.compute_aabb().unwrap().half_extents.x * t.scale.x;
                // The annulus's inner edge: the smallest vertex radius.
                let inner = match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
                    bevy::render::mesh::VertexAttributeValues::Float32x3(v) => v
                        .iter()
                        .map(|p| Vec2::new(p[0], p[1]).length())
                        .fold(f32::MAX, f32::min),
                    _ => panic!("annulus positions are Float32x3"),
                } * t.scale.x;
                (r, t, outer, outer - inner)
            })
            .collect()
    }

    fn cores(&mut self) -> Vec<(SlowBindRing, Transform, f32, f32)> {
        self.rings().into_iter().filter(|r| !r.0.halo).collect()
    }

    fn scuffs(&mut self) -> Vec<Transform> {
        let mut q = self.app.world_mut().query::<(&SlowScuff, &Transform)>();
        q.iter(self.app.world()).map(|(_, t)| *t).collect()
    }

    /// Every proc flash: (victim, school, ability).
    fn flashes(&mut self) -> Vec<(Entity, SpellSchool, AbilityType)> {
        let mut q = self.app.world_mut().query::<&SchoolImpact>();
        q.iter(self.app.world())
            .map(|i| (i.target, i.school, i.ability))
            .collect()
    }
}

// =============================================================================
// The moving gate
// =============================================================================

#[test]
fn a_moving_slowed_unit_wears_the_ring_at_its_feet() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0), vec![slow("Frostbolt", 0.7)]);
    h.walk(unit, Vec3::X, SLOWED_SPEED, 5);

    let cores = h.cores();
    assert_eq!(cores.len(), 1, "one ring per pulse, found {}", cores.len());
    let at = h.pos(unit);
    let (_, t, _, _) = &cores[0];
    assert!(
        (t.translation.xz() - at.xz()).length() < 1e-4,
        "the ring is at {:?}, the unit's feet are at {:?}",
        t.translation,
        at
    );
    assert!(
        (t.translation.y - SLOW_RING_Y).abs() < 1e-6,
        "the ring must lie on the floor, not at the sim y: {}",
        t.translation.y
    );
    // Flat: the annulus's normal points up.
    let normal = t.rotation * Vec3::Z;
    assert!(normal.dot(Vec3::Y) > 0.999, "ring normal {normal:?}");
}

#[test]
fn a_stationary_slowed_unit_shows_nothing() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0), vec![slow("Frostbolt", 0.7)]);
    // Checked EVERY frame across two and a half periods: a ring lives 0.4s of
    // each 1s period, so a single look at the end could fall in the gap.
    for frame in 0..((2.5 * SLOW_RING_PERIOD / DT) as usize) {
        h.idle(1);
        assert!(
            h.rings().is_empty(),
            "a standing slowed unit pulsed a ring (frame {frame})"
        );
        assert!(
            h.scuffs().is_empty(),
            "a standing slowed unit laid a scuff (frame {frame})"
        );
    }
    // The aura is really there and routed — the gate is what held it back.
    assert!(h.app.world().get::<SlowTrailEmitter>(unit).is_some());
}

#[test]
fn an_unslowed_moving_unit_shows_nothing() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0), vec![]);
    h.walk(unit, Vec3::X, 5.0, 60);
    assert!(h.rings().is_empty());
    assert!(h.scuffs().is_empty());
}

#[test]
fn a_unit_that_stops_stops_pulsing() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0), vec![slow("Frost Shock", 0.5)]);
    h.walk(unit, Vec3::X, 2.5, 60);
    assert!(!h.rings().is_empty() || !h.scuffs().is_empty());
    // Everything already drawn outlives the stop by at most its own life.
    h.idle((SLOW_SCUFF_LIFETIME.max(SLOW_RING_LIFETIME) / DT) as usize + 2);
    // And nothing new appears for two more periods — checked every frame.
    for frame in 0..((2.0 * SLOW_RING_PERIOD / DT) as usize) {
        h.idle(1);
        assert!(
            h.rings().is_empty(),
            "a unit that stopped kept pulsing (frame {frame})"
        );
        assert!(
            h.scuffs().is_empty(),
            "a unit that stopped kept scuffing (frame {frame})"
        );
    }
    // Walking again pulses at once, not on the next period boundary.
    h.walk(unit, Vec3::X, 2.5, 2);
    assert_eq!(h.cores().len(), 1);
}

#[test]
fn the_ring_pulses_once_per_period() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0), vec![slow("Frostbolt", 0.7)]);
    let mut pulses = 0;
    let mut live = 0;
    for _ in 0..((3.0 * SLOW_RING_PERIOD / DT) as usize) {
        h.walk(unit, Vec3::X, SLOWED_SPEED, 1);
        let now = h.cores().len();
        if now > live {
            pulses += now - live;
        }
        live = now;
    }
    assert_eq!(pulses, 3, "3s of walking is 3 pulses at 1 Hz, saw {pulses}");
}

// =============================================================================
// The ring as drawn
// =============================================================================

#[test]
fn the_ring_follows_the_victim() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0), vec![slow("Frostbolt", 0.7)]);
    h.walk(unit, Vec3::X, SLOWED_SPEED, 1);
    let born = h.cores()[0].1.translation;
    // Mid-life, the victim has moved on; the ring moved with it.
    h.walk(unit, Vec3::new(1.0, 0.0, 1.0), SLOWED_SPEED, 8);
    let now = h.cores()[0].1.translation;
    let at = h.pos(unit);
    assert!((now.xz() - at.xz()).length() < 1e-4);
    assert!(
        (now.xz() - born.xz()).length() > 0.5,
        "guard: the victim must actually have moved"
    );
}

#[test]
fn the_ring_grows_as_a_constant_width_band_and_fades() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, 1.0, 0.0), vec![slow("Frostbolt", 0.7)]);
    h.walk(unit, Vec3::X, SLOWED_SPEED, 1);
    let (_, _, r0, w0) = h.cores()[0];
    let mut last = (r0, w0);
    let mut alphas = vec![];
    // Hold still from here: the ring lives out its life, no new pulse comes.
    while let Some((ring, _, r, w)) = h.cores().first().copied() {
        last = (r, w);
        let mat = {
            let mut q = h
                .app
                .world_mut()
                .query::<(&SlowBindRing, &MeshMaterial3d<StandardMaterial>)>();
            q.iter(h.app.world())
                .find(|(r, _)| !r.halo)
                .map(|(_, m)| m.0.clone())
                .unwrap()
        };
        alphas.push((
            ring.age,
            h.app
                .world()
                .resource::<Assets<StandardMaterial>>()
                .get(&mat)
                .unwrap()
                .base_color
                .alpha(),
        ));
        h.idle(1);
    }
    let (r1, w1) = last;
    // Outer radius: born near the start diameter, dies near the end diameter.
    let r_start = 0.5 * SLOW_RING_DIAMETER_START + 0.5 * SLOW_RING_BAND_WIDTH;
    let r_end = 0.5 * SLOW_RING_DIAMETER_END + 0.5 * SLOW_RING_BAND_WIDTH;
    assert!(
        (r0 - r_start).abs() < 0.06,
        "born at radius {r0}, expected ~{r_start}"
    );
    assert!(
        (r1 - r_end).abs() < 0.08,
        "died at radius {r1}, expected ~{r_end}"
    );
    // The band is a stroke: its measured width does not grow with the ring.
    for w in [w0, w1] {
        assert!(
            (w - SLOW_RING_BAND_WIDTH).abs() < 0.01,
            "band width {w}, expected {SLOW_RING_BAND_WIDTH}"
        );
    }
    // Full alpha through the hold, then fading.
    let early = alphas.iter().find(|(age, _)| *age < 0.2).unwrap().1;
    let late = alphas.last().unwrap().1;
    assert!(
        late < 0.5 * early,
        "alpha {early} -> {late}: the ring never faded"
    );
    assert!(h.rings().is_empty(), "the ring outlived its life");
}

// =============================================================================
// The scuff
// =============================================================================

#[test]
fn the_scuff_is_laid_along_the_path_on_the_floor() {
    let mut h = Harness::new();
    let start = Vec3::new(0.0, 1.0, 0.0);
    let unit = h.spawn_unit(start, vec![slow("Crippling Poison", 0.3)]);
    // Crippling Poison: 1.5 yd/s. 0.4s of walking is 0.6 yd.
    h.walk(unit, Vec3::Z, 1.5, 20);
    let scuffs = h.scuffs();
    assert!(
        scuffs.len() >= 3,
        "0.6 yd walked laid {} segments",
        scuffs.len()
    );
    let end = h.pos(unit);
    for t in &scuffs {
        assert!((t.translation.y - SLOW_SCUFF_Y).abs() < 1e-6);
        assert!(
            t.translation.x.abs() < 1e-3,
            "off the path: {:?}",
            t.translation
        );
        assert!(t.translation.z > start.z && t.translation.z < end.z);
        // Laid along the travel direction: the quad's length axis (local X)
        // points down the path.
        let along = t.rotation * Vec3::X;
        assert!(along.dot(Vec3::Z).abs() > 0.999, "segment axis {along:?}");
    }
}

// =============================================================================
// Crippling Poison's proc flash
// =============================================================================

#[test]
fn crippling_poison_flashes_once_on_application() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(
        Vec3::new(0.0, 1.0, 0.0),
        vec![slow("Crippling Poison", 0.3)],
    );
    h.idle(10);
    let flashes = h.flashes();
    assert_eq!(
        flashes.len(),
        1,
        "one flash per application, got {}",
        flashes.len()
    );
    assert_eq!(
        flashes[0],
        (unit, SpellSchool::Nature, AbilityType::CripplingPoison)
    );
}

#[test]
fn other_slows_bring_no_flash() {
    let mut h = Harness::new();
    h.spawn_unit(Vec3::new(0.0, 1.0, 0.0), vec![slow("Frostbolt", 0.7)]);
    h.idle(10);
    assert!(h.flashes().is_empty());
}

// =============================================================================
// Routing
// =============================================================================

#[test]
fn every_slow_source_routes_to_its_tint() {
    let defs = AbilityDefinitions::default();
    let name = |a: AbilityType| defs.get(&a).unwrap().name.clone();
    assert_eq!(
        SlowTint::for_slow(&name(AbilityType::Frostbolt)),
        Some(SlowTint::Frost)
    );
    assert_eq!(
        SlowTint::for_slow(&name(AbilityType::FrostShock)),
        Some(SlowTint::Frost)
    );
    assert_eq!(
        SlowTint::for_slow(&name(AbilityType::ConcussiveShot)),
        Some(SlowTint::Physical)
    );
    assert_eq!(
        SlowTint::for_slow(&name(AbilityType::CripplingPoison)),
        Some(SlowTint::Nature)
    );
    // The two code-applied slows, through the names the sim actually stamps.
    let chill = frost_armor_movement_slow_aura();
    assert_eq!(chill.effect_type, AuraType::MovementSpeedSlow);
    assert_eq!(
        SlowTint::for_slow(&chill.ability_name),
        Some(SlowTint::Frost)
    );
    assert_eq!(
        SlowTint::for_slow(&frost_trap_zone_slow_name()),
        Some(SlowTint::Frost)
    );
    // Nothing else routes: the fallthrough is `None`, never a default tint.
    assert_eq!(SlowTint::for_slow("Hamstring"), None);
}

/// The name the Frost Trap zone stamps on the slow it applies, read off a real
/// run of `slow_zone_system` rather than restated, so a rename in `traps.rs`
/// fails the routing pin above instead of silently unrouting the zone.
fn frost_trap_zone_slow_name() -> String {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
    app.add_systems(Update, slow_zone_system);
    let hunter = app
        .world_mut()
        .spawn(Combatant::new(1, 0, CharacterClass::Hunter))
        .id();
    app.world_mut().spawn((
        SlowZone {
            owner_team: 1,
            owner: hunter,
            radius: 5.0,
            duration_remaining: 10.0,
            slow_magnitude: 0.4,
        },
        Transform::default(),
    ));
    let victim = app
        .world_mut()
        .spawn((
            Combatant::new(2, 0, CharacterClass::Warrior),
            Transform::from_xyz(1.0, 1.0, 0.0),
            ActiveAuras { auras: vec![] },
        ))
        .id();
    app.update();
    app.update();
    let auras = app.world().get::<ActiveAuras>(victim).unwrap();
    let zone_slow = auras
        .auras
        .iter()
        .find(|a| a.effect_type == AuraType::MovementSpeedSlow)
        .expect("guard: the zone must actually slow the victim");
    zone_slow.ability_name.clone()
}

#[test]
fn tints_come_from_the_school_authority_and_the_daze_is_hueless() {
    assert_eq!(SlowTint::Frost.color(), SpellSchool::Frost.color());
    assert_eq!(SlowTint::Nature.color(), SpellSchool::Nature.color());
    let physical = SlowTint::Physical.color().to_srgba();
    let spread = physical.red.max(physical.green).max(physical.blue)
        - physical.red.min(physical.green).min(physical.blue);
    assert!(spread < 0.1, "the daze must stay near-white: {physical:?}");
}

#[test]
fn the_ring_takes_the_hardest_slows_tint() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(
        Vec3::new(0.0, 1.0, 0.0),
        vec![slow("Frostbolt", 0.7), slow("Crippling Poison", 0.3)],
    );
    h.walk(unit, Vec3::X, 1.5, 2);
    let (ring, _, _, _) = h.cores()[0];
    assert_eq!(ring.tint, SlowTint::Nature.color());
}
