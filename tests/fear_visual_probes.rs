//! Probes for the shadow-husk fear treatment (`update_fear_visuals`).
//!
//! Appearance is not testable here — what is, and what has broken before in the
//! sibling polymorph system, is the RESTORE. Each exit path gets a probe: the
//! aura component removed outright (`update_auras` drops it when the last aura
//! expires), the vec emptied (damage break / dispel / sandbox teardown), and
//! death (aura processing skips dead combatants, so the aura outlives the
//! victim). Owner scoping covers two units feared at once, and the co-hold
//! probe covers the Fear+Polymorph arbitration (sheep wins).
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` — no window, no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, Combatant, FearShroud, FearedVisual, OriginalBodyMaterial,
    OriginalMesh, PolymorphedVisual, VisualBody, WalkAnim,
};
use arenasim::states::play_match::{update_fear_shroud, update_fear_visuals};
use arenasim::CharacterClass;

/// Fixed tick for the harness.
const TICK: Duration = Duration::from_millis(100);

/// A plain Fear aura (natural Fear / Psychic Scream).
fn fear_aura() -> Aura {
    Aura {
        effect_type: AuraType::Fear,
        duration: 8.0,
        magnitude: 0.0,
        break_on_damage_threshold: 0.0,
        ..Default::default()
    }
}

/// Death Coil's horror is a Fear-TYPE aura (bypasses `FearImmunity`), so the
/// treatment must key on the type and cover it (R8). Modeled as a Fear aura
/// with the non-breaking threshold horror uses.
fn horror_aura() -> Aura {
    Aura {
        effect_type: AuraType::Fear,
        duration: 3.0,
        magnitude: 0.0,
        break_on_damage_threshold: -1.0,
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
        app.add_systems(Update, (update_fear_visuals, update_fear_shroud).chain());
        Harness { app }
    }

    /// Spawn a combatant with a `VisualBody` child. Returns (unit, body,
    /// original_body_material_handle).
    fn spawn_unit(&mut self, team: u8, slot: u8) -> (Entity, Entity, Handle<StandardMaterial>) {
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
                Combatant::new(team, slot, CharacterClass::Warlock),
                WalkAnim { phase: 0.0, previous_xz: Vec2::ZERO, idle_time: 0.0 },
            ))
            .id();
        self.app.world_mut().entity_mut(unit).add_child(body);
        (unit, body, material)
    }

    fn body_material(&self, body: Entity) -> Handle<StandardMaterial> {
        self.app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(body)
            .unwrap()
            .0
            .clone()
    }

    fn shrouds_of(&mut self, owner: Entity) -> usize {
        self.app
            .world_mut()
            .query::<&FearShroud>()
            .iter(self.app.world())
            .filter(|s| s.owner == owner)
            .count()
    }

    fn total_shrouds(&mut self) -> usize {
        self.app.world_mut().query::<&FearShroud>().iter(self.app.world()).count()
    }

    fn stored_materials(&mut self) -> usize {
        self.app
            .world_mut()
            .query::<&OriginalBodyMaterial>()
            .iter(self.app.world())
            .count()
    }
}

/// R1: gaining Fear inserts `FearedVisual`, tints the body, stores the original
/// material, and spawns exactly one shroud — all within one tick.
#[test]
fn tints_on_fear() {
    let mut h = Harness::new();
    let (unit, body, original_material) = h.spawn_unit(1, 0);

    h.app.update();
    assert!(h.app.world().get::<FearedVisual>(unit).is_none(), "not feared yet");
    assert_eq!(h.shrouds_of(unit), 0, "no shroud before fear");

    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![fear_aura()] });
    h.app.update();
    assert!(h.app.world().get::<FearedVisual>(unit).is_some(), "FearedVisual inserted");
    assert_ne!(h.body_material(body), original_material, "husk tint applied");
    assert_eq!(h.stored_materials(), 1, "original body material stored");
    assert_eq!(
        h.app.world().get::<OriginalBodyMaterial>(body).unwrap().0,
        original_material,
        "stored handle is the true body material"
    );
    assert_eq!(h.shrouds_of(unit), 1, "exactly one shroud");

    // Idempotent while the aura holds.
    h.app.update();
    assert_eq!(h.shrouds_of(unit), 1, "shroud must not accumulate");
    assert_eq!(h.stored_materials(), 1, "stored material must not accumulate");
}

/// R7 / AE3 (component-removal trap): natural expiry removes the whole
/// `ActiveAuras` component (not just empties the vec) → treatment restored.
#[test]
fn restores_on_component_removal() {
    let mut h = Harness::new();
    let (unit, body, original_material) = h.spawn_unit(1, 0);
    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![fear_aura()] });
    h.app.update();
    assert!(h.app.world().get::<FearedVisual>(unit).is_some());

    h.app.world_mut().entity_mut(unit).remove::<ActiveAuras>();
    h.app.update();
    assert!(h.app.world().get::<FearedVisual>(unit).is_none(), "FearedVisual removed");
    assert_eq!(h.body_material(body), original_material, "true material restored");
    assert_eq!(h.stored_materials(), 0, "stored material removed");
    assert_eq!(h.shrouds_of(unit), 0, "shroud despawned");
}

/// R7 / AE1 (death trap): a killing blow leaves the aura on the corpse, but the
/// treatment must restore the same frame `is_alive()` goes false — no shroud on
/// the corpse.
#[test]
fn restores_on_death_with_aura_present() {
    let mut h = Harness::new();
    let (unit, body, original_material) = h.spawn_unit(1, 0);
    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![fear_aura()] });
    h.app.update();
    assert!(h.app.world().get::<FearedVisual>(unit).is_some());

    // Killing blow: the aura survives on the corpse.
    h.app.world_mut().get_mut::<Combatant>(unit).unwrap().current_health = 0.0;
    h.app.update();
    assert!(h.app.world().get::<ActiveAuras>(unit).is_some(), "aura still on the corpse");
    assert!(h.app.world().get::<FearedVisual>(unit).is_none(), "restored on death");
    assert_eq!(h.body_material(body), original_material, "material restored on corpse");
    assert_eq!(h.shrouds_of(unit), 0, "no shroud on the corpse");
}

/// R7: damage-break / dispel — the aura vec is emptied while the component
/// stays → treatment restored.
#[test]
fn restores_on_vec_emptied() {
    let mut h = Harness::new();
    let (unit, body, original_material) = h.spawn_unit(1, 0);
    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![fear_aura()] });
    h.app.update();
    assert!(h.app.world().get::<FearedVisual>(unit).is_some());

    // Damage break / dispel: aura removed but component present.
    h.app.world_mut().get_mut::<ActiveAuras>(unit).unwrap().auras.clear();
    h.app.update();
    assert!(h.app.world().get::<FearedVisual>(unit).is_none(), "restored on break");
    assert_eq!(h.body_material(body), original_material);
    assert_eq!(h.shrouds_of(unit), 0);
}

/// Non-accumulation: fear → restore → fear again yields exactly one shroud and
/// one stored material (no leak across repeats).
#[test]
fn no_accumulation_across_repeats() {
    let mut h = Harness::new();
    let (unit, _body, _) = h.spawn_unit(1, 0);

    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![fear_aura()] });
    h.app.update();
    h.app.world_mut().get_mut::<ActiveAuras>(unit).unwrap().auras.clear();
    h.app.update();
    // Re-fear.
    h.app.world_mut().get_mut::<ActiveAuras>(unit).unwrap().auras.push(fear_aura());
    h.app.update();

    assert_eq!(h.shrouds_of(unit), 1, "one shroud after re-fear");
    assert_eq!(h.stored_materials(), 1, "one stored material after re-fear");
}

/// Owner scoping: two feared units; one restores → only its own shroud
/// despawns.
#[test]
fn restore_is_owner_scoped() {
    let mut h = Harness::new();
    let (a, _, _) = h.spawn_unit(1, 0);
    let (b, _, _) = h.spawn_unit(2, 0);
    h.app.world_mut().entity_mut(a).insert(ActiveAuras { auras: vec![fear_aura()] });
    h.app.world_mut().entity_mut(b).insert(ActiveAuras { auras: vec![fear_aura()] });
    h.app.update();
    assert_eq!(h.shrouds_of(a), 1);
    assert_eq!(h.shrouds_of(b), 1);

    // A's fear ends by the vec emptying.
    h.app.world_mut().get_mut::<ActiveAuras>(a).unwrap().auras.clear();
    h.app.update();
    assert_eq!(h.shrouds_of(a), 0, "A's shroud stripped");
    assert_eq!(h.shrouds_of(b), 1, "B's shroud untouched");
    assert_eq!(h.total_shrouds(), 1, "only B's shroud remains");
    assert!(h.app.world().get::<FearedVisual>(b).is_some());
}

/// R8: Death Coil's horror is a Fear-type aura, so it gets the treatment.
#[test]
fn horror_gets_fear_treatment() {
    let mut h = Harness::new();
    let (unit, body, original_material) = h.spawn_unit(1, 0);
    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![horror_aura()] });
    h.app.update();
    assert!(h.app.world().get::<FearedVisual>(unit).is_some(), "horror is Fear-type → treated");
    assert_ne!(h.body_material(body), original_material, "husk tint applied for horror");
    assert_eq!(h.shrouds_of(unit), 1);
}

/// KTD1 co-hold: while polymorphed, a Fear aura applies NO fear treatment (the
/// sheep look wins — the fear query carries `Without<PolymorphedVisual>`). When
/// Polymorph ends with Fear still active, the fear treatment applies next tick.
#[test]
fn polymorph_wins_while_co_held() {
    let mut h = Harness::new();
    let (unit, _body, _) = h.spawn_unit(1, 0);
    // Stand in for an active polymorph without running its system.
    h.app.world_mut().entity_mut(unit).insert(PolymorphedVisual);
    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![fear_aura()] });
    h.app.update();
    assert!(
        h.app.world().get::<FearedVisual>(unit).is_none(),
        "no fear treatment while polymorphed"
    );
    assert_eq!(h.shrouds_of(unit), 0, "no shroud while polymorphed");
    assert_eq!(h.stored_materials(), 0, "fear did not touch the body material");

    // Polymorph ends; Fear is still active.
    h.app.world_mut().entity_mut(unit).remove::<PolymorphedVisual>();
    h.app.update();
    assert!(
        h.app.world().get::<FearedVisual>(unit).is_some(),
        "fear treatment applies once the sheep look lifts"
    );
    assert_eq!(h.shrouds_of(unit), 1);
}
