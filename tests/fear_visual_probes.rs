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
    ActiveAuras, Aura, AuraType, Combatant, FearFlash, FearMote, FearMoteEmitter, FearShroud,
    FearedVisual, OriginalBodyMaterial, OriginalMesh, PolymorphedVisual, VisualBody, WalkAnim,
};
use arenasim::states::play_match::{
    cleanup_fear_flashes, cleanup_fear_motes, update_fear_flashes, update_fear_mote_emitters,
    update_fear_motes, update_fear_run, update_fear_shroud, update_fear_visuals, update_sheep_hop,
    update_walk_animation,
};
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
        app.add_systems(
            Update,
            (
                update_fear_visuals,
                update_fear_shroud,
                // Mote trio (U3): emitter spawns, update floats/fades, cleanup
                // despawns. Chained so a spawned mote is not floated/cleaned up
                // the same tick it is born.
                update_fear_mote_emitters,
                update_fear_motes,
                cleanup_fear_motes,
                // Flash trio (U4): the apply/break flashes are spawned by
                // `update_fear_visuals` above; these tick their grow/fade and
                // despawn the expired ones. Chained so a just-spawned flash is
                // not cleaned up the same tick it is born.
                update_fear_flashes,
                cleanup_fear_flashes,
            )
                .chain(),
        );
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

    /// Live fear-mote count across the whole world (motes are unattached world
    /// particles, not owner-scoped).
    fn motes(&mut self) -> usize {
        self.app.world_mut().query::<&FearMote>().iter(self.app.world()).count()
    }

    /// Live fear-flash count across the whole world (flashes are unattached
    /// world bursts, spawned by both transition branches of `update_fear_visuals`).
    fn flashes(&mut self) -> usize {
        self.app.world_mut().query::<&FearFlash>().iter(self.app.world()).count()
    }

    /// The set of live fear-flash entities — lets a probe prove a NEW flash was
    /// spawned by a transition, independent of whether older flashes have expired.
    fn flash_entities(&mut self) -> std::collections::HashSet<Entity> {
        self.app
            .world_mut()
            .query_filtered::<Entity, With<FearFlash>>()
            .iter(self.app.world())
            .collect()
    }

    /// Cumulative motes the unit's emitter has spawned (the cadence counter).
    /// Frozen once `FearedVisual` is gone (the emitter stops being iterated),
    /// which is exactly what proves emitter-stops-on-restore.
    fn motes_spawned(&self, unit: Entity) -> u32 {
        self.app
            .world()
            .get::<FearMoteEmitter>(unit)
            .map(|e| e.motes_spawned)
            .unwrap_or(0)
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

// ---------------------------------------------------------------------------
// U2: panic-run gait with composed tremble (R2, R4, KTD2).
//
// These probes exercise the gait systems in `rendering/effects/gait.rs`, not
// the treatment-owning systems above. The body's local `Transform.y` is the
// only observable — appearance is untestable, but the WAVEFORM (cadence /
// amplitude / time-driven vs distance-driven) and the one-writer arbitration
// are. A separate, lighter harness: gait needs only `WalkAnim`, `Transform`,
// `Combatant`, and a `VisualBody` child — no meshes or materials.
// ---------------------------------------------------------------------------

/// Distance moved per tick when driving the distance-based gaits. Comfortably
/// above `WALK_IDLE_EPSILON` so phase advances every frame.
const GAIT_STEP: f32 = 0.15;

/// An app with `MinimalPlugins` (for `Time`) and a manual clock, ready for gait
/// systems to be registered onto.
fn gait_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
    app
}

/// Spawn a unit (`WalkAnim` + `Combatant` + `Transform`) with a `VisualBody`
/// child at `rest_y = 0`. Returns `(unit, body)`.
fn spawn_gait_unit(app: &mut App) -> (Entity, Entity) {
    let body = app
        .world_mut()
        .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
        .id();
    let unit = app
        .world_mut()
        .spawn((
            Transform::from_xyz(0.0, 1.0, 0.0),
            Combatant::new(1, 0, CharacterClass::Warlock),
            WalkAnim { phase: 0.0, previous_xz: Vec2::ZERO, idle_time: 0.0 },
        ))
        .id();
    app.world_mut().entity_mut(unit).add_child(body);
    (unit, body)
}

fn body_y(app: &App, body: Entity) -> f32 {
    app.world().get::<Transform>(body).unwrap().translation.y
}

fn move_unit(app: &mut App, unit: Entity, dx: f32) {
    app.world_mut().get_mut::<Transform>(unit).unwrap().translation.x += dx;
}

/// Run all three gaits registered together, moving one unit `dx` per tick for
/// `ticks` frames; return the body's local Y after each frame. The optional
/// marker selects which gait owns the body (`None` = walk).
fn moving_gait_trace(marker: Option<&'static str>, ticks: usize) -> Vec<f32> {
    let mut app = gait_app();
    app.add_systems(
        Update,
        (update_walk_animation, update_sheep_hop, update_fear_run),
    );
    let (unit, body) = spawn_gait_unit(&mut app);
    match marker {
        Some("fear") => {
            app.world_mut().entity_mut(unit).insert(FearedVisual);
        }
        Some("poly") => {
            app.world_mut().entity_mut(unit).insert(PolymorphedVisual);
        }
        _ => {}
    }
    let mut ys = Vec::with_capacity(ticks);
    for _ in 0..ticks {
        move_unit(&mut app, unit, GAIT_STEP);
        app.update();
        ys.push(body_y(&app, body));
    }
    ys
}

/// R4: over a fixed moving window, the fear gait's body-Y trace differs
/// measurably from both the walk bob and the sheep hop — distinct cadence and
/// amplitude, not the same waveform under a different marker.
#[test]
fn fear_gait_distinct_from_walk_and_hop() {
    let walk = moving_gait_trace(None, 30);
    let hop = moving_gait_trace(Some("poly"), 30);
    let fear = moving_gait_trace(Some("fear"), 30);

    let max_abs_diff = |a: &[f32], b: &[f32]| {
        a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max)
    };

    let vs_walk = max_abs_diff(&fear, &walk);
    let vs_hop = max_abs_diff(&fear, &hop);
    assert!(vs_walk > 0.03, "fear gait must differ from the walk bob (max diff {vs_walk})");
    assert!(vs_hop > 0.03, "fear gait must differ from the sheep hop (max diff {vs_hop})");
}

/// R2 (the load-bearing fixed-timestep-strobe assertion): a STATIONARY feared
/// unit still trembles. With zero sim displacement the distance-driven bob
/// contributes nothing, so any body-Y variation is the time-driven tremble
/// alone — proving it rides the wall clock, not "sim moved this frame".
#[test]
fn stationary_feared_unit_still_trembles() {
    let mut app = gait_app();
    app.add_systems(Update, update_fear_run);
    let (unit, body) = spawn_gait_unit(&mut app);
    app.world_mut().entity_mut(unit).insert(FearedVisual);

    let start_x = app.world().get::<Transform>(unit).unwrap().translation.x;

    let mut ys = Vec::new();
    for _ in 0..30 {
        // Deliberately never move the unit.
        app.update();
        ys.push(body_y(&app, body));
    }

    let end_x = app.world().get::<Transform>(unit).unwrap().translation.x;
    assert_eq!(start_x, end_x, "the unit must be stationary — zero sim displacement");

    let min = ys.iter().cloned().fold(f32::MAX, f32::min);
    let max = ys.iter().cloned().fold(f32::MIN, f32::max);
    assert!(
        max - min > 0.02,
        "a stationary feared unit must still tremble over time (body-Y range {})",
        max - min
    );
}

/// One-writer invariant (query side): a `FearedVisual` unit is excluded from
/// `update_walk_animation`'s query. With ONLY the walk system running, a moving
/// feared unit's body is never written (stays at rest), while a normal unit's
/// body bobs.
#[test]
fn feared_unit_excluded_from_walk_query() {
    let mut app = gait_app();
    app.add_systems(Update, update_walk_animation);
    let (normal, normal_body) = spawn_gait_unit(&mut app);
    let (feared, feared_body) = spawn_gait_unit(&mut app);
    app.world_mut().entity_mut(feared).insert(FearedVisual);

    let mut normal_bobbed = false;
    for _ in 0..10 {
        move_unit(&mut app, normal, GAIT_STEP);
        move_unit(&mut app, feared, GAIT_STEP);
        app.update();
        if body_y(&app, normal_body).abs() > 1e-6 {
            normal_bobbed = true;
        }
        assert_eq!(
            body_y(&app, feared_body),
            0.0,
            "feared unit is excluded from the walk query — its body stays untouched"
        );
    }
    assert!(normal_bobbed, "the walk system bobs a non-feared moving unit");
}

/// One-writer invariant (co-hold): a unit with BOTH `FearedVisual` and
/// `PolymorphedVisual` is driven by the hop only — `update_fear_run` carries
/// `Without<PolymorphedVisual>` and never touches it, while the hop does.
#[test]
fn co_held_unit_driven_by_hop_not_fear_run() {
    // fear_run alone must leave the co-held unit untouched.
    let mut app = gait_app();
    app.add_systems(Update, update_fear_run);
    let (unit, body) = spawn_gait_unit(&mut app);
    app.world_mut().entity_mut(unit).insert(FearedVisual);
    app.world_mut().entity_mut(unit).insert(PolymorphedVisual);
    for _ in 0..10 {
        move_unit(&mut app, unit, GAIT_STEP);
        app.update();
        assert_eq!(
            body_y(&app, body),
            0.0,
            "update_fear_run must exclude a co-held (polymorphed) unit"
        );
    }

    // The hop alone DOES drive the co-held unit.
    let mut app2 = gait_app();
    app2.add_systems(Update, update_sheep_hop);
    let (unit2, body2) = spawn_gait_unit(&mut app2);
    app2.world_mut().entity_mut(unit2).insert(FearedVisual);
    app2.world_mut().entity_mut(unit2).insert(PolymorphedVisual);
    let mut hopped = false;
    for _ in 0..10 {
        move_unit(&mut app2, unit2, GAIT_STEP);
        app2.update();
        if body_y(&app2, body2).abs() > 1e-6 {
            hopped = true;
        }
    }
    assert!(hopped, "the sheep hop drives the co-held unit");
}

/// Restore / no residual: after `FearedVisual` is removed, the walk gait
/// resumes and eases the body back to its rest baseline. The Y self-zeroes via
/// the shared writer — no frozen tremble offset persists (KTD2).
#[test]
fn restore_leaves_no_residual_offset() {
    let mut app = gait_app();
    app.add_systems(
        Update,
        (update_walk_animation, update_sheep_hop, update_fear_run),
    );
    let (unit, body) = spawn_gait_unit(&mut app);
    app.world_mut().entity_mut(unit).insert(FearedVisual);

    // Feared and moving → the body carries a live composed offset.
    for _ in 0..10 {
        move_unit(&mut app, unit, GAIT_STEP);
        app.update();
    }
    assert!(
        body_y(&app, body).abs() > 1e-6,
        "the fear gait produced a live body offset"
    );

    // Fear breaks; the unit stops. The walk gait must ease back to rest.
    app.world_mut().entity_mut(unit).remove::<FearedVisual>();
    for _ in 0..25 {
        app.update(); // stationary
    }
    assert!(
        body_y(&app, body).abs() < 1e-3,
        "body returns to its rest baseline after restore — no residual offset (got {})",
        body_y(&app, body)
    );
}

// ---------------------------------------------------------------------------
// U3: rising fear-motes emitter (R3 / KTD3).
//
// Appearance is untestable; what is observable — and what these probes pin —
// is the CADENCE (motes spawn on the interval, not every tick and not never),
// the LIFETIME BOUND (each mote self-expires, so the live count stays bounded
// under a long feared window instead of growing without limit), and
// EMITTER-STOPS-ON-RESTORE (removing `FearedVisual` freezes the spawn counter
// and lets in-flight motes drain to zero with no orphans). These run on the
// main `Harness`, which now registers the mote trio alongside the treatment.
// ---------------------------------------------------------------------------

/// R3: while `FearedVisual` holds, motes spawn on roughly the fixed interval
/// (~1 per 0.5s at the 0.1s tick — far below one-per-tick, far above never),
/// and the live count stays bounded because each mote self-expires. Over 5s of
/// fear ~10 motes are spawned in total, yet only a handful are ever alive at
/// once.
#[test]
fn motes_spawn_on_cadence_and_stay_bounded() {
    let mut h = Harness::new();
    let (unit, _body, _) = h.spawn_unit(1, 0);
    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![fear_aura()] });

    // 50 ticks = 5.0s of fear at the 0.1s harness tick.
    let mut peak_live = 0usize;
    for _ in 0..50 {
        h.app.update();
        peak_live = peak_live.max(h.motes());
    }

    // Cadence: interval 0.5s over ~5s feared → ~10 spawns, minus ~1 arming
    // tick. Anything in [8, 11] proves it fires on the interval rather than
    // every tick (which would be ~50) or never (0).
    let spawned = h.motes_spawned(unit);
    assert!(
        (8..=11).contains(&spawned),
        "expected ~10 motes spawned over 5s of fear, got {spawned}"
    );

    // Lifetime bound: mote lifetime 1.2s / interval 0.5s caps concurrency at 3
    // (a 4th would mean expiry is not firing). The peak live count over the
    // whole run must never exceed that — no unbounded growth.
    assert!(
        peak_live <= 3,
        "live mote count must stay bounded (<=3); peaked at {peak_live}"
    );
    assert!(peak_live >= 1, "at least one mote should be alive mid-fear");
}

/// Emitter-stops-on-restore: once `FearedVisual` is removed the spawn counter
/// freezes (no new motes), and every in-flight mote finishes its own lifetime
/// and despawns — no orphans, no leak.
#[test]
fn motes_stop_on_restore_and_drain_to_zero() {
    let mut h = Harness::new();
    let (unit, _body, _) = h.spawn_unit(1, 0);
    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![fear_aura()] });

    // Fear for 2s → emitter armed, some motes in flight.
    for _ in 0..20 {
        h.app.update();
    }
    let spawned_before = h.motes_spawned(unit);
    assert!(spawned_before >= 2, "emitter should have spawned motes while feared");
    assert!(h.motes() >= 1, "motes in flight while feared");

    // Fear breaks (aura vec emptied). `update_fear_visuals` removes
    // `FearedVisual`, so the emitter stops being iterated.
    h.app.world_mut().get_mut::<ActiveAuras>(unit).unwrap().auras.clear();
    h.app.update();
    assert!(h.app.world().get::<FearedVisual>(unit).is_none(), "restored");

    // Run well past a mote lifetime (1.2s → 12 ticks; 20 gives margin).
    for _ in 0..20 {
        h.app.update();
    }

    // No new motes were spawned after restore (counter frozen)...
    assert_eq!(
        h.motes_spawned(unit),
        spawned_before,
        "emitter must not spawn any mote after FearedVisual is removed"
    );
    // ...and every in-flight mote self-expired — no orphans left behind.
    assert_eq!(h.motes(), 0, "all motes drained to zero after restore");
}

// ---------------------------------------------------------------------------
// U4: apply / break shadow flash (R5, R6 / AE2).
//
// Appearance is untestable; what is observable — and what these probes pin — is
// that a flash entity is spawned on BOTH transitions (apply and break), that the
// break flash fires even inside a sub-second window (AE2: Fear breaks on any
// damage), and that the flashes self-expire so their count stays bounded across
// repeated fear cycles (no accumulation). The main `Harness` now registers the
// flash trio alongside the treatment and mote systems.
// ---------------------------------------------------------------------------

/// R5: Fear landing spawns an apply flash.
#[test]
fn apply_flash_spawns_on_fear() {
    let mut h = Harness::new();
    let (unit, _body, _) = h.spawn_unit(1, 0);
    assert_eq!(h.flashes(), 0, "no flash before fear");

    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![fear_aura()] });
    h.app.update();
    assert!(h.app.world().get::<FearedVisual>(unit).is_some(), "unit is feared");
    assert!(h.flashes() >= 1, "an apply flash is spawned when Fear lands");
}

/// AE2 / R6: Fear broken 0.4s after it lands still spawns a break flash — the
/// flash mechanism fires inside a sub-second window. The break flash is
/// identified as a NEW flash entity (not present before the break), so the
/// assertion holds regardless of whether the apply flash has already expired.
#[test]
fn break_flash_spawns_in_sub_second_window() {
    let mut h = Harness::new();
    let (unit, _body, _) = h.spawn_unit(1, 0);
    h.app.world_mut().entity_mut(unit).insert(ActiveAuras { auras: vec![fear_aura()] });
    h.app.update(); // apply

    // ~0.4s pass (4 ticks at the 0.1s harness tick) — a sub-second fear window.
    for _ in 0..4 {
        h.app.update();
    }
    let before = h.flash_entities();

    // Damage break: aura vec emptied. `update_fear_visuals` takes the
    // transition-out branch and spawns a break flash.
    h.app.world_mut().get_mut::<ActiveAuras>(unit).unwrap().auras.clear();
    h.app.update();
    assert!(h.app.world().get::<FearedVisual>(unit).is_none(), "restored");

    let after = h.flash_entities();
    let new_flashes = after.difference(&before).count();
    assert!(
        new_flashes >= 1,
        "a break flash spawns even when Fear breaks inside a sub-second window \
         (new flashes: {new_flashes})"
    );
}

/// Flashes self-expire: over repeated apply/break cycles the live `FearFlash`
/// count stays bounded — the flashes are removed by cleanup, they do not
/// accumulate on the corpse of the effect.
#[test]
fn flashes_self_expire_and_stay_bounded() {
    let mut h = Harness::new();
    let (unit, _body, _) = h.spawn_unit(1, 0);

    let mut peak = 0usize;
    for _ in 0..5 {
        // Apply.
        h.app
            .world_mut()
            .entity_mut(unit)
            .insert(ActiveAuras { auras: vec![fear_aura()] });
        for _ in 0..6 {
            h.app.update();
            peak = peak.max(h.flashes());
        }
        // Break.
        h.app.world_mut().get_mut::<ActiveAuras>(unit).unwrap().auras.clear();
        for _ in 0..6 {
            h.app.update();
            peak = peak.max(h.flashes());
        }
        h.app.world_mut().entity_mut(unit).remove::<ActiveAuras>();
    }

    // Each cycle spawns at most an apply + a break flash, each ~0.4s-lived
    // (4 ticks). With 6 ticks between transitions they never pile up: the live
    // count stays small and bounded — the flashes are being cleaned up.
    assert!(peak <= 3, "flash count must stay bounded across cycles (peaked at {peak})");
    // And after a long idle they have all drained.
    for _ in 0..10 {
        h.app.update();
    }
    assert_eq!(h.flashes(), 0, "all flashes self-expired");
}
