//! Probes for the Druid control treatments (`druid_control.rs`): Entangling
//! Roots' bark roots and Cyclone's funnel with its lifted victim.
//!
//! Geometry, not bookkeeping: the probes read WORLD positions off
//! `GlobalTransform` — where the roots reach, where the funnel's strips sit,
//! how high the victim's body actually is — and only fall back on a component
//! to say a treatment is gone. Every exit path is covered for both: the aura
//! component removed (`update_auras` drops it when the last aura expires), the
//! vec emptied (dispel / damage break / sandbox teardown), and death (aura
//! processing skips the dead, so the aura outlives the victim). Cyclone also
//! gets the composition with a Fear that landed first, since the Cyclone's
//! immunity blocks only NEW auras.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window, no
//! GPU. `TransformPlugin` is load-bearing: it is what turns the lift into a
//! world height the probes can read.

use std::f32::consts::TAU;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::transform::TransformSystem;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraPending, AuraType, CcKind, CcRig, Combatant, OriginalBodyMaterial,
    OriginalMesh, Pet, PetType, RootStyle, RootedVisual, VisualBody, WalkAnim,
};
use arenasim::states::play_match::{
    apply_cyclone_lift, billboard_cc_beads, cleanup_cc_flares, cleanup_cc_rigs, root_growth,
    root_style, strip_point, update_cc_flares, update_cc_rigs, update_cyclone_funnels,
    update_cyclone_visuals, update_druid_motes, update_fear_run, update_fear_visuals,
    update_hard_cc_visuals, update_root_tendrils, update_walk_animation, CycloneBand,
    CycloneFunnel, CycloneLift, DruidMote, RootSegment, CYCLONE_EASE_SECS, CYCLONE_VICTIM_BOB,
    CYCLONE_VICTIM_LIFT, ROOTS_BIRTH_SECS, ROOTS_COUNT, ROOTS_DEATH_SECS, ROOTS_HEIGHT,
    ROOTS_RING_RADIUS, ROOT_SEGMENTS,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(50);
const DT: f32 = 0.05;
/// A combatant's sim y, which is also its body's rendered centre (`rest_y` 0).
const BODY_Y: f32 = 1.0;

fn ticks(secs: f32) -> u32 {
    (secs / DT).ceil() as u32
}

/// An aura exactly as `process_casting` would land it, from the real config.
fn aura_from_config(ability: AbilityType) -> Aura {
    let defs = AbilityDefinitions::default();
    let def = defs.get(&ability).expect("ability is in abilities.ron");
    AuraPending::from_ability(Entity::PLACEHOLDER, Entity::PLACEHOLDER, def)
        .expect("ability applies an aura")
        .aura
}

fn fear_aura() -> Aura {
    Aura {
        effect_type: AuraType::Fear,
        duration: 8.0,
        break_on_damage_threshold: 0.0,
        ..Default::default()
    }
}

struct Harness {
    app: App,
}

impl Harness {
    /// With the gaits and the fear treatment registered, as the client has
    /// them, so the lift is proved to compose with their absolute writes.
    fn new() -> Self {
        Self::build(true)
    }

    /// No other writer of the body's transform at all, so a lift that
    /// accumulated frame over frame would show.
    fn without_gaits() -> Self {
        Self::build(false)
    }

    fn build(gaits: bool) -> Self {
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
        if gaits {
            app.add_systems(
                Update,
                (update_fear_visuals, update_walk_animation, update_fear_run)
                    .chain()
                    .before(update_hard_cc_visuals),
            );
        }
        // Same order as the real registration in `states/mod.rs`.
        app.add_systems(
            Update,
            (
                update_hard_cc_visuals,
                update_cc_rigs,
                billboard_cc_beads,
                update_cc_flares,
                cleanup_cc_rigs,
                cleanup_cc_flares,
                update_root_tendrils,
                update_cyclone_visuals,
                update_cyclone_funnels,
                update_druid_motes,
            )
                .chain(),
        );
        app.add_systems(
            PostUpdate,
            apply_cyclone_lift.before(TransformSystem::TransformPropagate),
        );
        Harness { app }
    }

    /// A combatant with a `VisualBody` child, mirroring the real hierarchy.
    /// Returns (unit, body).
    fn spawn_unit(&mut self, x: f32, z: f32) -> (Entity, Entity) {
        let mesh = self
            .app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Capsule3d::new(0.5, 1.5));
        let material = self
            .app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let body = self
            .app
            .world_mut()
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                OriginalMesh(mesh),
                VisualBody { rest_y: 0.0 },
                Transform::default(),
            ))
            .id();
        let unit = self
            .app
            .world_mut()
            .spawn((
                Transform::from_xyz(x, BODY_Y, z),
                Combatant::new(1, 0, CharacterClass::Warrior),
                WalkAnim {
                    phase: 0.0,
                    previous_xz: Vec2::new(x, z),
                    idle_time: 0.0,
                    body_offset: 0.0,
                },
            ))
            .id();
        self.app.world_mut().entity_mut(unit).add_child(body);
        (unit, body)
    }

    /// A pet with the real geometry from `spawn_pet`: sim y 1.75, body
    /// rendered at world 0.3.
    fn spawn_pet(&mut self) -> (Entity, Entity) {
        const PET_SIM_Y: f32 = 1.75;
        const PET_MESH_Y: f32 = 0.3;
        let mesh = self
            .app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Capsule3d::new(0.35, 0.6));
        let material = self
            .app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let body = self
            .app
            .world_mut()
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                OriginalMesh(mesh),
                VisualBody {
                    rest_y: PET_MESH_Y - PET_SIM_Y,
                },
                Transform::from_xyz(0.0, PET_MESH_Y - PET_SIM_Y, 0.0),
            ))
            .id();
        let owner = self.app.world_mut().spawn_empty().id();
        let pet = self
            .app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, PET_SIM_Y, 0.0),
                Combatant::new(1, 1, CharacterClass::Hunter),
                Pet {
                    owner,
                    pet_type: PetType::Spider,
                },
                WalkAnim {
                    phase: 0.0,
                    previous_xz: Vec2::ZERO,
                    idle_time: 0.0,
                    body_offset: 0.0,
                },
            ))
            .id();
        self.app.world_mut().entity_mut(pet).add_child(body);
        (pet, body)
    }

    fn apply(&mut self, unit: Entity, aura: Aura) {
        let mut e = self.app.world_mut().entity_mut(unit);
        match e.get_mut::<ActiveAuras>() {
            Some(mut active) => active.auras.push(aura),
            None => {
                e.insert(ActiveAuras { auras: vec![aura] });
            }
        }
    }

    fn remove_aura_type(&mut self, unit: Entity, t: AuraType) {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(unit)
            .unwrap()
            .auras
            .retain(|a| a.effect_type != t);
    }

    fn clear_auras(&mut self, unit: Entity) {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(unit)
            .unwrap()
            .auras
            .clear();
    }

    fn remove_auras(&mut self, unit: Entity) {
        self.app
            .world_mut()
            .entity_mut(unit)
            .remove::<ActiveAuras>();
    }

    fn kill(&mut self, unit: Entity) {
        self.app
            .world_mut()
            .get_mut::<Combatant>(unit)
            .unwrap()
            .current_health = 0.0;
    }

    fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.app.update();
        }
    }

    fn world_of(&self, e: Entity) -> GlobalTransform {
        *self.app.world().get::<GlobalTransform>(e).unwrap()
    }

    fn body_height(&self, body: Entity) -> f32 {
        self.world_of(body).translation().y
    }

    fn body_rotation(&self, body: Entity) -> Quat {
        self.world_of(body).compute_transform().rotation
    }

    fn all<C: Component>(&mut self) -> Vec<Entity> {
        let mut q = self.app.world_mut().query_filtered::<Entity, With<C>>();
        q.iter(self.app.world()).collect()
    }

    fn root_rigs(&mut self, owner: Entity) -> Vec<Entity> {
        let mut q = self.app.world_mut().query::<(Entity, &CcRig)>();
        q.iter(self.app.world())
            .filter(|(_, r)| r.owner == owner && r.kind == CcKind::Root)
            .map(|(e, _)| e)
            .collect()
    }

    /// World positions of both ends of every root segment.
    fn root_points(&mut self) -> Vec<Vec3> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&RootSegment, &GlobalTransform)>();
        q.iter(self.app.world())
            .flat_map(|(_, g)| {
                [
                    g.transform_point(Vec3::new(0.0, -0.5, 0.0)),
                    g.transform_point(Vec3::new(0.0, 0.5, 0.0)),
                ]
            })
            .collect()
    }

    /// The highest point any root reaches, above the floor.
    fn root_reach(&mut self) -> f32 {
        self.root_points()
            .into_iter()
            .map(|p| p.y)
            .fold(0.0, f32::max)
    }

    fn funnels_of(&mut self, owner: Entity) -> Vec<(Entity, Option<f32>)> {
        let mut q = self.app.world_mut().query::<(Entity, &CycloneFunnel)>();
        q.iter(self.app.world())
            .filter(|(_, f)| f.owner == owner)
            .map(|(e, f)| (e, f.retract))
            .collect()
    }

    /// World points along the bottom and top edges of every funnel strip.
    fn strip_points(&mut self) -> Vec<Vec3> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&CycloneBand, &GlobalTransform)>();
        let mut out = Vec::new();
        for (_, g) in q.iter(self.app.world()) {
            for i in 0..=10 {
                let u = i as f32 / 10.0;
                out.push(g.transform_point(strip_point(u, false)));
                out.push(g.transform_point(strip_point(u, true)));
            }
        }
        out
    }
}

// ==============================================================================
// Entangling Roots
// ==============================================================================

#[test]
fn entangling_roots_wear_bark_roots_and_spider_web_keeps_its_silk() {
    // The routing, on the auras the real config lands.
    assert_eq!(
        root_style(&aura_from_config(AbilityType::EntanglingRoots)),
        RootStyle::Roots
    );
    assert_eq!(
        root_style(&aura_from_config(AbilityType::SpiderWeb)),
        RootStyle::Web,
        "Spider Web shares the Nature school and must keep its silk"
    );
    assert_eq!(
        root_style(&aura_from_config(AbilityType::FrostNova)),
        RootStyle::Ice
    );

    // And the rig each builds.
    let mut h = Harness::new();
    let (roots_victim, _) = h.spawn_unit(0.0, 0.0);
    let (web_victim, _) = h.spawn_unit(10.0, 0.0);
    h.apply(roots_victim, aura_from_config(AbilityType::EntanglingRoots));
    h.apply(web_victim, aura_from_config(AbilityType::SpiderWeb));
    h.tick(1);

    let style = |h: &Harness, e| h.app.world().get::<RootedVisual>(e).map(|m| m.style);
    assert_eq!(style(&h, roots_victim), Some(RootStyle::Roots));
    assert_eq!(style(&h, web_victim), Some(RootStyle::Web));
    assert_eq!(
        h.all::<RootSegment>().len(),
        ROOTS_COUNT * ROOT_SEGMENTS,
        "only the Entangling Roots victim grows bark roots"
    );
}

#[test]
fn roots_grow_up_round_the_legs_over_the_birth_and_hold() {
    let mut h = Harness::new();
    let (unit, _) = h.spawn_unit(3.0, -2.0);
    h.apply(unit, aura_from_config(AbilityType::EntanglingRoots));

    h.tick(ticks(0.3));
    let early = h.root_reach();
    h.tick(ticks(ROOTS_BIRTH_SECS));
    let grown = h.root_reach();
    h.tick(ticks(2.0));
    let held = h.root_reach();

    assert!(
        early < grown * 0.85,
        "the roots must still be coming up 0.3s in ({early:.2} vs grown {grown:.2})"
    );
    // The tallest root's tip: ROOTS_HEIGHT x (0.85 + 0.3 sin) peaks near 1.1 x 1.15.
    assert!(
        grown > ROOTS_HEIGHT * 0.9 && grown < ROOTS_HEIGHT * 1.3,
        "grown roots reach {grown:.2}yd, expected about the shins-to-hips {ROOTS_HEIGHT}yd"
    );
    assert!(
        (held - grown).abs() < 1e-4,
        "the roots hold still once grown ({grown:.3} -> {held:.3})"
    );

    // Around the victim's legs, not somewhere else: every point within the
    // ring's outer radius of the victim's axis, and every base on the floor.
    for p in h.root_points() {
        let d = Vec2::new(p.x - 3.0, p.z + 2.0).length();
        assert!(
            d < ROOTS_RING_RADIUS * 1.25 + 0.1,
            "a root strays {d:.2}yd from the victim"
        );
        assert!(p.y > 0.0 && p.y < ROOTS_HEIGHT * 1.3);
    }
}

#[test]
fn root_dust_and_pebbles_play_during_the_birth_only() {
    let mut h = Harness::new();
    let (unit, _) = h.spawn_unit(0.0, 0.0);
    h.apply(unit, aura_from_config(AbilityType::EntanglingRoots));

    h.tick(ticks(0.8));
    let during = h.all::<DruidMote>().len();
    assert!(
        during > 10,
        "the birth kicks up dust and pebbles ({during})"
    );

    // Past the birth plus the longest mote life (pebbles, 3s): nothing left,
    // and nothing new while the roots hold.
    h.tick(ticks(ROOTS_BIRTH_SECS + 3.2));
    assert_eq!(
        h.all::<DruidMote>().len(),
        0,
        "no debris while the roots hold"
    );
}

#[test]
fn pebbles_come_to_rest_on_the_floor() {
    let mut h = Harness::new();
    let (unit, _) = h.spawn_unit(0.0, 0.0);
    h.apply(unit, aura_from_config(AbilityType::EntanglingRoots));
    h.tick(ticks(2.5));
    let mut q = h.app.world_mut().query::<(&DruidMote, &GlobalTransform)>();
    for (_, g) in q.iter(h.app.world()) {
        assert!(g.translation().y >= 0.0, "a mote fell through the floor");
    }
}

/// Every exit withdraws the roots back into the ground over the death sequence
/// and then removes them.
fn assert_roots_withdraw(exit: impl Fn(&mut Harness, Entity)) {
    let mut h = Harness::new();
    let (unit, _) = h.spawn_unit(0.0, 0.0);
    h.apply(unit, aura_from_config(AbilityType::EntanglingRoots));
    h.tick(ticks(ROOTS_BIRTH_SECS + 0.5));
    let grown = h.root_reach();

    exit(&mut h, unit);
    let mut last = grown;
    let mut halfway = None;
    for i in 0..ticks(ROOTS_DEATH_SECS) - 1 {
        h.tick(1);
        let reach = h.root_reach();
        assert!(
            reach <= last + 1e-4,
            "the roots must only sink once withdrawn ({last:.3} -> {reach:.3})"
        );
        last = reach;
        if i == ticks(ROOTS_DEATH_SECS * 0.5) {
            halfway = Some(reach);
        }
        assert_eq!(h.root_rigs(unit).len(), 1, "the rig plays out its death");
    }
    let halfway = halfway.unwrap();
    assert!(
        halfway < grown * 0.95 && halfway > 0.0,
        "halfway through the death the roots are partly down ({halfway:.2} of {grown:.2})"
    );
    h.tick(3);
    assert!(
        h.root_rigs(unit).is_empty(),
        "the rig is gone after the death"
    );
    assert!(h.all::<RootSegment>().is_empty());
    assert!(h.app.world().get::<RootedVisual>(unit).is_none());
}

#[test]
fn roots_withdraw_on_expiry() {
    assert_roots_withdraw(|h, u| h.remove_auras(u));
}

#[test]
fn roots_withdraw_on_dispel_or_break() {
    assert_roots_withdraw(|h, u| h.clear_auras(u));
}

#[test]
fn roots_withdraw_on_death() {
    assert_roots_withdraw(|h, u| h.kill(u));
}

#[test]
fn a_root_broken_mid_birth_sinks_from_where_it_reached() {
    let full = root_growth(ROOTS_BIRTH_SECS, None);
    let partial = root_growth(0.4, None);
    assert!(partial < full);
    // Armed at 0.4s: it starts at the partial height and never rises.
    let mut last = partial;
    for i in 0..=26 {
        let r = i as f32 * 0.05;
        let g = root_growth(0.4 + r, Some(r));
        assert!(g <= last + 1e-6, "rose after the break at r={r}");
        last = g;
    }
    assert_eq!(
        root_growth(0.4 + ROOTS_DEATH_SECS, Some(ROOTS_DEATH_SECS)),
        0.0
    );
}

// ==============================================================================
// Cyclone: the funnel
// ==============================================================================

#[test]
fn the_funnel_wraps_the_victim_narrow_at_the_waist_and_wide_at_the_top() {
    let mut h = Harness::new();
    let (unit, _) = h.spawn_unit(-4.0, 6.0);
    h.apply(unit, aura_from_config(AbilityType::Cyclone));
    h.tick(ticks(3.5));

    assert_eq!(h.funnels_of(unit).len(), 1, "one funnel per cycloned unit");
    let points = h.strip_points();
    assert!(points.len() > 100, "the funnel is full of strips");
    // Centred on the victim, and enclosing it.
    let radius = |p: Vec3| Vec2::new(p.x + 4.0, p.z - 6.0).length();
    let centre = points.iter().fold(Vec3::ZERO, |a, p| a + *p) / points.len() as f32;
    assert!(
        radius(centre) < 0.6,
        "the funnel is centred on its victim ({:.2} off)",
        radius(centre)
    );

    // The body bands (P1+P2) are the funnel's shape: where each one's midline
    // sits, and how far it is from the axis there. They rise and shrink to a
    // waist, then flare as they climb on.
    let mut q = h
        .app
        .world_mut()
        .query::<(&CycloneBand, &GlobalTransform)>();
    let mids: Vec<Vec3> = q
        .iter(h.app.world())
        .filter(|(b, _)| b.layer == 1)
        .map(|(_, g)| {
            (g.transform_point(strip_point(0.5, false)) + g.transform_point(strip_point(0.5, true)))
                * 0.5
        })
        .collect();
    let widest_in = |lo: f32, hi: f32| {
        mids.iter()
            .filter(|p| p.y >= lo && p.y < hi)
            .map(|p| radius(*p))
            .fold(0.0, f32::max)
    };
    let foot = widest_in(0.0, 1.2);
    let waist = widest_in(2.8, 3.6);
    let top = widest_in(4.5, 8.0);
    assert!(
        waist > 0.3,
        "the waist still encloses the victim ({waist:.2})"
    );
    assert!(
        top > waist * 2.0,
        "the funnel flares at the top ({top:.2}) far wider than its waist ({waist:.2})"
    );
    assert!(
        foot > waist,
        "the foot ({foot:.2}) is wider than the waist ({waist:.2})"
    );
}

#[test]
fn the_funnel_spins_on_the_wall_clock_over_a_still_victim() {
    let mut h = Harness::new();
    let (unit, _) = h.spawn_unit(0.0, 0.0);
    h.apply(unit, aura_from_config(AbilityType::Cyclone));
    h.tick(2);
    let (hub, _) = h.funnels_of(unit)[0];
    let a = h.world_of(hub).compute_transform().rotation;
    h.tick(1);
    let b = h.world_of(hub).compute_transform().rotation;
    assert!(a.angle_between(b) > 0.1, "the funnel turns every frame");
}

fn assert_funnel_ends(exit: impl Fn(&mut Harness, Entity)) {
    let mut h = Harness::new();
    let (unit, _) = h.spawn_unit(0.0, 0.0);
    h.apply(unit, aura_from_config(AbilityType::Cyclone));
    h.tick(ticks(2.0));
    exit(&mut h, unit);
    h.tick(1);
    let funnels = h.funnels_of(unit);
    assert_eq!(funnels.len(), 1);
    assert!(funnels[0].1.is_some(), "the funnel starts to fade on exit");
    h.tick(ticks(CYCLONE_EASE_SECS) + 2);
    assert!(h.funnels_of(unit).is_empty(), "the funnel is gone");
    assert!(h.all::<CycloneBand>().is_empty(), "with every strip");
}

#[test]
fn the_funnel_ends_on_expiry() {
    assert_funnel_ends(|h, u| h.remove_auras(u));
}

#[test]
fn the_funnel_ends_when_the_aura_is_removed() {
    assert_funnel_ends(|h, u| h.clear_auras(u));
}

#[test]
fn the_funnel_ends_on_death() {
    assert_funnel_ends(|h, u| h.kill(u));
}

// ==============================================================================
// Cyclone: the lifted victim
// ==============================================================================

/// Lifts the victim, then ends the Cyclone by `exit` and checks the body comes
/// all the way back down, upright and facing forward.
fn assert_victim_lands(exit: impl Fn(&mut Harness, Entity)) {
    let mut h = Harness::new();
    let (unit, body) = h.spawn_unit(2.0, 2.0);
    h.tick(2);
    assert!((h.body_height(body) - BODY_Y).abs() < 1e-5);

    h.apply(unit, aura_from_config(AbilityType::Cyclone));
    h.tick(ticks(1.5));
    let lifted = h.body_height(body) - BODY_Y;
    assert!(
        lifted > CYCLONE_VICTIM_LIFT - CYCLONE_VICTIM_BOB - 0.01
            && lifted < CYCLONE_VICTIM_LIFT + CYCLONE_VICTIM_BOB + 0.01,
        "a cycloned body floats about {CYCLONE_VICTIM_LIFT}yd up (got {lifted:.3})"
    );

    exit(&mut h, unit);
    // Down within the ease; the spin may take longer to wind down to forward.
    h.tick(ticks(CYCLONE_EASE_SECS) + 1);
    assert!(
        (h.body_height(body) - BODY_Y).abs() < 1e-4,
        "the body is back on the ground after the ease ({:.4})",
        h.body_height(body)
    );
    h.tick(ticks(3.5));
    assert!(
        (h.body_height(body) - BODY_Y).abs() < 1e-5,
        "and stays there"
    );
    assert!(
        h.body_rotation(body).angle_between(Quat::IDENTITY) < 1e-3,
        "the body ends facing forward, upright"
    );
    assert!(
        h.app.world().get::<CycloneLift>(body).is_none(),
        "the lift is gone once played out"
    );
    // Nothing is left behind in the shared restore slots.
    assert!(h.app.world().get::<OriginalBodyMaterial>(body).is_none());
}

#[test]
fn the_victim_lands_on_expiry() {
    assert_victim_lands(|h, u| h.remove_auras(u));
}

#[test]
fn the_victim_lands_when_the_aura_is_removed() {
    assert_victim_lands(|h, u| h.clear_auras(u));
}

#[test]
fn the_victim_lands_on_death() {
    assert_victim_lands(|h, u| h.kill(u));
}

#[test]
fn the_victim_eases_up_rather_than_popping() {
    let mut h = Harness::new();
    let (unit, body) = h.spawn_unit(0.0, 0.0);
    h.apply(unit, aura_from_config(AbilityType::Cyclone));
    let mut last = BODY_Y;
    for _ in 0..ticks(CYCLONE_EASE_SECS) {
        h.tick(1);
        let y = h.body_height(body);
        assert!(
            y - last < CYCLONE_VICTIM_LIFT * 0.25,
            "the lift jumped {:.3}yd in one frame",
            y - last
        );
        last = y;
    }
    assert!(
        last - BODY_Y > CYCLONE_VICTIM_LIFT * 0.7,
        "and is up by the end of the ease"
    );
}

#[test]
fn the_victim_spins_slowly_while_held() {
    let mut h = Harness::new();
    let (unit, body) = h.spawn_unit(0.0, 0.0);
    h.apply(unit, aura_from_config(AbilityType::Cyclone));
    h.tick(ticks(1.0));
    let a = h.body_rotation(body);
    h.tick(1);
    let b = h.body_rotation(body);
    let step = a.angle_between(b);
    // 0.75 rev/s over one 50ms frame.
    let expected = 0.75 * TAU * DT;
    assert!(
        (step - expected).abs() < 0.02,
        "spins {step:.3} rad a frame, expected {expected:.3}"
    );
    // About the vertical: the body stays upright.
    assert!((b * Vec3::Y - Vec3::Y).length() < 1e-4);
}

#[test]
fn the_lift_never_accumulates_without_another_writer() {
    let mut h = Harness::without_gaits();
    let (unit, body) = h.spawn_unit(0.0, 0.0);
    h.apply(unit, aura_from_config(AbilityType::Cyclone));
    let mut highest: f32 = 0.0;
    for _ in 0..ticks(5.0) {
        h.tick(1);
        highest = highest.max(h.body_height(body) - BODY_Y);
    }
    assert!(
        highest <= CYCLONE_VICTIM_LIFT + CYCLONE_VICTIM_BOB + 1e-3,
        "the lift crept up to {highest:.3}yd"
    );
    h.remove_auras(unit);
    h.tick(ticks(4.0));
    assert!((h.body_height(body) - BODY_Y).abs() < 1e-5);
}

#[test]
fn a_cycloned_pet_floats_by_its_stature() {
    let mut h = Harness::new();
    let (pet, body) = h.spawn_pet();
    h.tick(2);
    let rest = h.body_height(body);
    h.apply(pet, aura_from_config(AbilityType::Cyclone));
    h.tick(ticks(1.5));
    let lifted = h.body_height(body) - rest;
    let expected = CYCLONE_VICTIM_LIFT * 0.55;
    assert!(
        (lifted - expected).abs() <= CYCLONE_VICTIM_BOB * 0.55 + 0.01,
        "a pet lifts {lifted:.3}, expected about {expected:.3}"
    );
}

#[test]
fn a_re_cyclone_during_the_descent_lifts_again_with_one_funnel() {
    let mut h = Harness::new();
    let (unit, body) = h.spawn_unit(0.0, 0.0);
    h.apply(unit, aura_from_config(AbilityType::Cyclone));
    h.tick(ticks(1.5));
    h.remove_auras(unit);
    h.tick(ticks(0.3));
    h.apply(unit, aura_from_config(AbilityType::Cyclone));
    h.tick(ticks(1.0));
    assert!(h.body_height(body) - BODY_Y > CYCLONE_VICTIM_LIFT - CYCLONE_VICTIM_BOB - 0.01);
    let held: Vec<_> = h
        .funnels_of(unit)
        .into_iter()
        .filter(|(_, r)| r.is_none())
        .collect();
    assert_eq!(held.len(), 1, "exactly one live funnel");
}

// ==============================================================================
// Cyclone over a Fear that landed first
// ==============================================================================

fn material_of(h: &Harness, body: Entity) -> Handle<StandardMaterial> {
    h.app
        .world()
        .get::<MeshMaterial3d<StandardMaterial>>(body)
        .unwrap()
        .0
        .clone()
}

#[test]
fn a_fear_already_on_the_target_keeps_its_husk_on_the_lifted_body() {
    let mut h = Harness::new();
    let (unit, body) = h.spawn_unit(0.0, 0.0);
    let own = material_of(&h, body);
    let mesh = h.app.world().get::<Mesh3d>(body).unwrap().0.clone();

    h.apply(unit, fear_aura());
    h.tick(2);
    let husk = material_of(&h, body);
    assert_ne!(husk, own, "the fear husk is on");

    h.apply(unit, aura_from_config(AbilityType::Cyclone));
    h.tick(ticks(1.5));
    // The fear run's tremble (±0.04) rides on the lift, as it rides any gait.
    assert!(
        h.body_height(body) - BODY_Y > 0.2,
        "the feared body is lifted"
    );
    assert_eq!(
        material_of(&h, body),
        husk,
        "the Cyclone leaves the husk alone"
    );
    assert_eq!(
        h.app
            .world()
            .get::<OriginalBodyMaterial>(body)
            .map(|m| m.0.clone()),
        Some(own.clone()),
        "Fear still owns the restore slot, holding the body's own material"
    );
    assert_eq!(h.app.world().get::<Mesh3d>(body).unwrap().0, mesh);

    // The Cyclone ends first: down, still a husk.
    h.remove_aura_type(unit, AuraType::Cyclone);
    h.tick(ticks(4.0));
    assert!(
        (h.body_height(body) - BODY_Y).abs() < 0.05,
        "back on the ground"
    );
    assert_eq!(material_of(&h, body), husk, "still feared");

    // Then the Fear: the body's own material comes back.
    h.remove_aura_type(unit, AuraType::Fear);
    h.tick(2);
    assert_eq!(material_of(&h, body), own);
    assert!(h.app.world().get::<OriginalBodyMaterial>(body).is_none());
}

#[test]
fn a_fear_that_ends_inside_the_cyclone_restores_the_body_mid_air() {
    let mut h = Harness::new();
    let (unit, body) = h.spawn_unit(0.0, 0.0);
    let own = material_of(&h, body);

    h.apply(unit, fear_aura());
    h.tick(2);
    h.apply(unit, aura_from_config(AbilityType::Cyclone));
    h.tick(ticks(1.0));

    h.remove_aura_type(unit, AuraType::Fear);
    h.tick(2);
    assert_eq!(material_of(&h, body), own, "the Fear restores its slot");
    assert!(
        h.body_height(body) - BODY_Y > 0.2,
        "while the Cyclone keeps the body up"
    );

    h.remove_aura_type(unit, AuraType::Cyclone);
    h.tick(ticks(4.0));
    assert!((h.body_height(body) - BODY_Y).abs() < 0.05);
    assert_eq!(material_of(&h, body), own);
}

// ==============================================================================
// The spin's wind-down
// ==============================================================================

/// Wherever the Cyclone ends, the spin decelerates forward — never reversing —
/// and comes to rest exactly facing forward.
#[test]
fn the_spin_winds_down_forward_to_facing_front() {
    for end_at in [0.31_f32, 0.9, 1.33, 2.0, 2.6] {
        let mut h = Harness::without_gaits();
        let (unit, body) = h.spawn_unit(0.0, 0.0);
        h.apply(unit, aura_from_config(AbilityType::Cyclone));
        h.tick(ticks(end_at));
        h.remove_auras(unit);
        let mut last_yaw = h.app.world().get::<CycloneLift>(body).unwrap().yaw;
        let mut unwrapped = 0.0;
        for _ in 0..ticks(4.0) {
            h.tick(1);
            let Some(lift) = h.app.world().get::<CycloneLift>(body) else {
                break;
            };
            let step = (lift.yaw - last_yaw).rem_euclid(TAU);
            assert!(step < 1.0, "the spin jumped or reversed (step {step:.3})");
            unwrapped += step;
            last_yaw = lift.yaw;
        }
        assert!(unwrapped >= 0.0);
        assert!(h.app.world().get::<CycloneLift>(body).is_none());
        assert!(h.body_rotation(body).angle_between(Quat::IDENTITY) < 1e-3);
    }
}
