//! Probes for the sheep-form body swap (`update_polymorph_visuals`).
//!
//! Appearance is not testable here — what is, and what has broken before, is
//! the RESTORE. Each exit path gets a probe: the aura component removed
//! outright (`update_auras` drops it when the last aura expires), the vec
//! emptied (the animation sandbox's teardown), and death (aura processing
//! skips dead combatants, so the aura outlives the victim). The owner-scoping
//! probe covers two units polymorphed at once.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` — no window, no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, Combatant, OriginalMesh, PolymorphedVisual, SheepPart,
    TransformPuff, VisualBody, WalkAnim,
};
use arenasim::states::play_match::{update_polymorph_visuals, update_sheep_hop};
use arenasim::CharacterClass;

/// Fixed tick for the harness. Real time barely advances between `update()`
/// calls in a test, which would leave the hop's idle clock (0.1s) unreachable.
const TICK: Duration = Duration::from_millis(100);

fn poly_aura() -> Aura {
    Aura {
        effect_type: AuraType::Polymorph,
        duration: 8.0,
        magnitude: 0.0,
        break_on_damage_threshold: 0.0,
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
        // Chained: the hop only drives units the swap has already marked.
        app.add_systems(Update, (update_polymorph_visuals, update_sheep_hop).chain());
        Harness { app }
    }

    fn spawn_unit(&mut self, team: u8, slot: u8) -> (Entity, Entity, Handle<Mesh>, Handle<StandardMaterial>) {
        let mesh = self.app.world_mut().resource_mut::<Assets<Mesh>>().add(Capsule3d::new(0.5, 1.5));
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
                MeshMaterial3d(material.clone()),
                OriginalMesh(mesh.clone()),
                VisualBody { rest_y: 0.0 },
                Transform::default(),
            ))
            .id();
        let unit = self
            .app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, 1.0, 0.0),
                Combatant::new(team, slot, CharacterClass::Mage),
                WalkAnim { phase: 0.0, previous_xz: Vec2::ZERO, idle_time: 0.0 },
            ))
            .id();
        self.app.world_mut().entity_mut(unit).add_child(body);
        (unit, body, mesh, material)
    }

    fn puffs(&mut self) -> usize {
        self.app.world_mut().query::<&TransformPuff>().iter(self.app.world()).count()
    }

    fn sheep_parts_of(&mut self, owner: Entity) -> usize {
        self.app
            .world_mut()
            .query::<&SheepPart>()
            .iter(self.app.world())
            .filter(|p| p.owner == owner)
            .count()
    }
}

#[test]
fn transforms_and_restores() {
    let mut h = Harness::new();
    let (unit, body, original_mesh, original_material) = h.spawn_unit(1, 0);

    h.app.update();
    assert_eq!(h.sheep_parts_of(unit), 0, "no parts before polymorph");

    // Apply
    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![poly_aura()] });
    h.app.update();
    assert!(h.app.world().get::<PolymorphedVisual>(unit).is_some());
    assert_ne!(h.app.world().get::<Mesh3d>(body).unwrap().0, original_mesh, "torso swapped");
    assert_ne!(
        h.app.world().get::<MeshMaterial3d<StandardMaterial>>(body).unwrap().0,
        original_material,
        "wool coat applied"
    );
    let parts = h.sheep_parts_of(unit);
    assert!(parts >= 8, "expected head/muzzle/ears/legs/tail, got {parts}");
    assert_eq!(h.puffs(), 1, "one puff marks the transform-in");

    // Idempotent while the aura holds
    h.app.update();
    assert_eq!(h.sheep_parts_of(unit), parts, "parts must not accumulate");
    assert_eq!(h.puffs(), 1, "puffs must not accumulate either");

    // Exit path: component REMOVED (what update_auras does when the vec empties)
    h.app.world_mut().entity_mut(unit).remove::<ActiveAuras>();
    h.app.update();
    assert!(h.app.world().get::<PolymorphedVisual>(unit).is_none());
    assert_eq!(h.app.world().get::<Mesh3d>(body).unwrap().0, original_mesh, "capsule restored");
    assert_eq!(
        h.app.world().get::<MeshMaterial3d<StandardMaterial>>(body).unwrap().0,
        original_material,
        "class material restored"
    );
    assert_eq!(h.sheep_parts_of(unit), 0, "parts despawned");
    assert_eq!(h.puffs(), 2, "a second puff marks the restore");
}

#[test]
fn restores_on_death_with_aura_still_present() {
    let mut h = Harness::new();
    let (unit, body, original_mesh, _) = h.spawn_unit(1, 0);
    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![poly_aura()] });
    h.app.update();
    assert!(h.app.world().get::<PolymorphedVisual>(unit).is_some());

    // Killing blow: the aura survives on the corpse.
    h.app.world_mut().get_mut::<Combatant>(unit).unwrap().current_health = 0.0;
    h.app.update();
    assert!(h.app.world().get::<ActiveAuras>(unit).is_some(), "aura still on the corpse");
    assert!(h.app.world().get::<PolymorphedVisual>(unit).is_none(), "restored on death");
    assert_eq!(h.app.world().get::<Mesh3d>(body).unwrap().0, original_mesh);
    assert_eq!(h.sheep_parts_of(unit), 0);
}

#[test]
fn restore_is_owner_scoped() {
    let mut h = Harness::new();
    let (a, _, _, _) = h.spawn_unit(1, 0);
    let (b, _, _, _) = h.spawn_unit(2, 0);
    h.app.world_mut().entity_mut(a).insert(ActiveAuras { auras: vec![poly_aura()] });
    h.app.world_mut().entity_mut(b).insert(ActiveAuras { auras: vec![poly_aura()] });
    h.app.update();
    let b_parts = h.sheep_parts_of(b);
    assert!(b_parts > 0);

    // A's polymorph ends by the vec emptying (the sandbox teardown path).
    h.app.world_mut().get_mut::<ActiveAuras>(a).unwrap().auras.clear();
    h.app.update();
    assert_eq!(h.sheep_parts_of(a), 0, "A stripped");
    assert_eq!(h.sheep_parts_of(b), b_parts, "B untouched");
    assert!(h.app.world().get::<PolymorphedVisual>(b).is_some());
}

#[test]
fn hop_lifts_a_moving_sheep_and_settles_a_still_one() {
    let mut h = Harness::new();
    let (unit, body, _, _) = h.spawn_unit(1, 0);
    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![poly_aura()] });
    h.app.update();
    assert!(h.app.world().get::<PolymorphedVisual>(unit).is_some());

    // Walk it. The hop is distance-driven, so travel is what raises the body —
    // a quarter of a hop step per tick puts it through a full arc.
    let mut peak = f32::MIN;
    for tick in 1..=6 {
        h.app.world_mut().get_mut::<Transform>(unit).unwrap().translation.x = tick as f32 * 0.225;
        h.app.update();
        peak = peak.max(h.app.world().get::<Transform>(body).unwrap().translation.y);
    }
    assert!(peak > 0.15, "hop should clear the walk bob's 0.10, got {peak}");

    // The shared `WalkAnim` baseline must track the sheep's position, or the
    // walk bob resumes on a stale delta when the polymorph breaks.
    let walk = h.app.world().get::<WalkAnim>(unit).unwrap();
    assert!((walk.previous_xz.x - 6.0 * 0.225).abs() < 1e-4, "baseline left stale");

    // Stand still: idle after 0.1s, then ease back to `rest_y` (0.0 here).
    for _ in 0..20 {
        h.app.update();
    }
    let resting = h.app.world().get::<Transform>(body).unwrap().translation.y;
    assert!(resting.abs() < 0.01, "a stationary sheep should settle to rest, got {resting}");
}

#[test]
fn sheep_stands_on_the_floor() {
    let mut h = Harness::new();
    let (unit, _, _, _) = h.spawn_unit(1, 0);
    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![poly_aura()] });
    h.app.update();

    // Parent y = 1.0, rest_y = 0.0, so world height = 1.0 + local y.
    let mut centers: Vec<f32> = Vec::new();
    let mut q = h.app.world_mut().query::<(&SheepPart, &Transform)>();
    for (_, t) in q.iter(h.app.world()) {
        centers.push(1.0 + t.translation.y);
    }
    centers.sort_by(f32::total_cmp);
    let lowest = centers[0];
    let highest = *centers.last().unwrap();
    // Legs are 0.30 long with their origin at the middle, so a centre at 0.15
    // means their feet touch the floor.
    assert!((lowest - 0.15).abs() < 0.01, "legs should stand on the floor, got {lowest}");
    assert!(highest < 1.0, "sheep should stay well under the capsule top, got {highest}");
}
