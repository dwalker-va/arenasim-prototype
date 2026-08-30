//! Probes for the hard-CC receiver treatment (`update_hard_cc_visuals`).
//!
//! Appearance is not testable here — what is, and what has broken before in the
//! sibling polymorph and fear systems, is the RESTORE. Each exit path gets a
//! probe for BOTH markers: the aura component removed outright (`update_auras`
//! drops it when the last aura expires), the vec emptied (damage break / dispel
//! / sandbox teardown), and death (aura processing skips dead combatants, so the
//! aura outlives the victim).
//!
//! Beyond the exit paths, the load-bearing probes here are the CO-HOLD (Root and
//! Stun occupy disjoint space and must both show, unlike Fear/Polymorph which
//! arbitrate), owner scoping, the wall-clock spin (the fixed-timestep strobe
//! guard), and the Boar Charge timing floor.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` — no window, no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::{AbilityType, SpellSchool};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, CcFlare, CcKind, CcRig, Combatant, NovaFreezeDelay, OriginalMesh,
    Pet, PetType, RootStyle, RootedVisual, StunnedVisual, VisualBody, WalkAnim,
};
use arenasim::states::play_match::{
    billboard_cc_beads, cc_envelope, cleanup_cc_flares, cleanup_cc_rigs, nova_outer_radius, root_style,
    update_cc_flares, update_cc_rigs,
    update_hard_cc_visuals,
};
use arenasim::CharacterClass;

/// Finer than the 100ms sibling files: the stun's 0.14s grow and 0.18s retract
/// need the resolution.
const TICK: Duration = Duration::from_millis(50);

/// Frost Nova: Root, 6s, Frost school, breaks at 80 cumulative damage.
fn frost_root() -> Aura {
    Aura {
        effect_type: AuraType::Root,
        duration: 6.0,
        magnitude: 1.0,
        break_on_damage_threshold: 80.0,
        spell_school: Some(SpellSchool::Frost),
        ..Default::default()
    }
}

/// Spider Web: Root, 4s, Nature school — the variant that must build silk
/// rather than crystals.
fn web_root() -> Aura {
    Aura {
        effect_type: AuraType::Root,
        duration: 4.0,
        magnitude: 1.0,
        break_on_damage_threshold: 80.0,
        spell_school: Some(SpellSchool::Nature),
        ..Default::default()
    }
}

/// Kidney Shot: Stun, 6s, never breaks on damage.
fn stun_aura() -> Aura {
    Aura {
        effect_type: AuraType::Stun,
        duration: 6.0,
        magnitude: 1.0,
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
        // The stun beads are textured quads, so the sparkle generator needs an
        // `Assets<Image>` to live in.
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
        // Same order as the real registration in `states/mod.rs`. There is no
        // camera in this harness, so `billboard_cc_beads` early-returns — it is
        // registered anyway to keep the two orders identical.
        app.add_systems(
            Update,
            (
                update_hard_cc_visuals,
                update_cc_rigs,
                billboard_cc_beads,
                update_cc_flares,
                cleanup_cc_rigs,
                cleanup_cc_flares,
            )
                .chain(),
        );
        Harness { app }
    }

    /// A combatant with a `VisualBody` child, mirroring the real hierarchy.
    fn spawn_unit(&mut self, team: u8, slot: u8) -> Entity {
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
                Transform::from_xyz(0.0, 1.0, 0.0),
                Combatant::new(team, slot, CharacterClass::Rogue),
                WalkAnim { phase: 0.0, previous_xz: Vec2::ZERO, idle_time: 0.0 },
            ))
            .id();
        self.app.world_mut().entity_mut(unit).add_child(body);
        unit
    }

    /// A pet, with the REAL geometry from `spawn_pet`: the sim entity sits at
    /// `owner_position + 0.75` (so world y 1.75 beside a combatant at 1.0) while
    /// the `VisualBody` child carries `rest_y = 0.3 - 1.75` and renders at world
    /// 0.3. Anchoring the whirl off the sim y instead of the body would hang it
    /// ~1.9yd above the pet's head.
    fn spawn_pet(&mut self, owner: Entity) -> Entity {
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
                VisualBody { rest_y: PET_MESH_Y - PET_SIM_Y },
                Transform::from_xyz(0.0, PET_MESH_Y - PET_SIM_Y, 0.0),
            ))
            .id();
        let pet = self
            .app
            .world_mut()
            .spawn((
                Transform::from_xyz(0.0, PET_SIM_Y, 0.0),
                Combatant::new(0, 1, CharacterClass::Hunter),
                Pet { owner, pet_type: PetType::Spider },
                WalkAnim { phase: 0.0, previous_xz: Vec2::ZERO, idle_time: 0.0 },
            ))
            .id();
        self.app.world_mut().entity_mut(pet).add_child(body);
        pet
    }

    fn rig_y(&mut self, rig: Entity) -> f32 {
        self.app.world().get::<Transform>(rig).unwrap().translation.y
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

    fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.app.update();
        }
    }

    fn rigs(&mut self) -> Vec<(Entity, Entity, CcKind, Option<f32>)> {
        let mut q = self.app.world_mut().query::<(Entity, &CcRig)>();
        q.iter(self.app.world())
            .map(|(e, r)| (e, r.owner, r.kind, r.retract))
            .collect()
    }

    fn rigs_of(&mut self, owner: Entity, kind: CcKind) -> Vec<(Entity, Option<f32>)> {
        self.rigs()
            .into_iter()
            .filter(|(_, o, k, _)| *o == owner && *k == kind)
            .map(|(e, _, _, r)| (e, r))
            .collect()
    }

    fn rig_children(&mut self, rig: Entity) -> usize {
        self.app
            .world()
            .get::<Children>(rig)
            .map(|c| c.len())
            .unwrap_or(0)
    }

    fn flares(&mut self) -> usize {
        self.app.world_mut().query::<&CcFlare>().iter(self.app.world()).count()
    }

    fn rig_rotation(&mut self, rig: Entity) -> Quat {
        self.app.world().get::<Transform>(rig).unwrap().rotation
    }

    fn rig_scale(&mut self, rig: Entity) -> f32 {
        self.app.world().get::<Transform>(rig).unwrap().scale.y
    }

    fn kill(&mut self, unit: Entity) {
        self.app
            .world_mut()
            .get_mut::<Combatant>(unit)
            .unwrap()
            .current_health = 0.0;
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
        self.app.world_mut().entity_mut(unit).remove::<ActiveAuras>();
    }

    fn has_root(&self, unit: Entity) -> bool {
        self.app.world().get::<RootedVisual>(unit).is_some()
    }

    fn root_style_of(&self, unit: Entity) -> Option<RootStyle> {
        self.app.world().get::<RootedVisual>(unit).map(|m| m.style)
    }

    fn has_stun(&self, unit: Entity) -> bool {
        self.app.world().get::<StunnedVisual>(unit).is_some()
    }
}

// ==============================================================================
// Transform-in
// ==============================================================================

#[test]
fn frost_root_spawns_crystals_and_one_flare() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, frost_root());
    h.tick(1);

    assert!(h.has_root(unit), "RootedVisual must be inserted");
    assert_eq!(h.root_style_of(unit), Some(RootStyle::Ice));
    let rigs = h.rigs_of(unit, CcKind::Root);
    assert_eq!(rigs.len(), 1, "exactly one root rig");
    // ROOT_SPIKE_COUNT crystals, no claim ring.
    assert_eq!(h.rig_children(rigs[0].0), 8, "eight ice crystals");
    assert_eq!(h.flares(), 1, "one apply flare per victim");
}

#[test]
fn web_root_builds_silk_not_crystals() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, web_root());
    h.tick(1);

    assert_eq!(h.root_style_of(unit), Some(RootStyle::Web));
    let rigs = h.rigs_of(unit, CcKind::Root);
    // WEB_SPOKES * WEB_SPOKE_SEGS + WEB_RINGS * WEB_SPOKES = 11*6 + 4*11 = 110.
    assert_eq!(h.rig_children(rigs[0].0), 110, "spokes plus crossing rings");
}

#[test]
fn stun_spawns_the_whirl_and_one_flare() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, stun_aura());
    h.tick(1);

    assert!(h.has_stun(unit));
    let rigs = h.rigs_of(unit, CcKind::Stun);
    assert_eq!(rigs.len(), 1);
    // STUN_ARMS * STUN_BEADS_PER_ARM, each a glowing core plus its halo shell.
    assert_eq!(h.rig_children(rigs[0].0), 10, "ten sparkles");
    assert_eq!(h.flares(), 1);
}

// ==============================================================================
// Non-accumulation
// ==============================================================================

#[test]
fn no_accumulation_while_held() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, frost_root());
    h.apply(unit, stun_aura());
    h.tick(1);
    let before: Vec<_> = h
        .rigs_of(unit, CcKind::Root)
        .into_iter()
        .chain(h.rigs_of(unit, CcKind::Stun))
        .collect();
    assert_eq!(before.len(), 2);

    h.tick(40);
    let after: Vec<_> = h
        .rigs_of(unit, CcKind::Root)
        .into_iter()
        .chain(h.rigs_of(unit, CcKind::Stun))
        .collect();
    assert_eq!(after.len(), 2, "held CC must not spawn a second rig per tick");
}

#[test]
fn no_accumulation_across_repeats() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);

    for cycle in 0..5 {
        h.apply(unit, stun_aura());
        h.tick(4);
        assert_eq!(
            h.rigs_of(unit, CcKind::Stun).len(),
            1,
            "cycle {cycle}: exactly one rig while held"
        );
        h.clear_auras(unit);
        // Past STUN_RETRACT_SECS so cleanup drains it before the next apply.
        h.tick(10);
        assert_eq!(
            h.rigs_of(unit, CcKind::Stun).len(),
            0,
            "cycle {cycle}: rig drained before re-apply"
        );
    }
    assert!(!h.has_stun(unit));
}

// ==============================================================================
// The four exits, for both markers
// ==============================================================================

#[test]
fn root_restores_on_component_removal() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, frost_root());
    h.tick(2);
    assert!(h.has_root(unit));

    // `update_auras` REMOVES the component when the last aura expires.
    h.remove_auras(unit);
    h.tick(1);
    assert!(!h.has_root(unit), "natural expiry must restore");
    assert!(h.rigs_of(unit, CcKind::Root)[0].1.is_some(), "retract armed");
}

#[test]
fn stun_restores_on_component_removal() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, stun_aura());
    h.tick(2);
    assert!(h.has_stun(unit));

    h.remove_auras(unit);
    h.tick(1);
    assert!(!h.has_stun(unit));
    assert!(h.rigs_of(unit, CcKind::Stun)[0].1.is_some());
}

#[test]
fn root_restores_on_vec_emptied() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, frost_root());
    h.tick(2);

    // Damage break / dispel / sandbox teardown all empty the vec in place.
    h.clear_auras(unit);
    h.tick(1);
    assert!(!h.has_root(unit));
    assert!(h.rigs_of(unit, CcKind::Root)[0].1.is_some());
}

#[test]
fn stun_restores_on_vec_emptied() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, stun_aura());
    h.tick(2);

    h.clear_auras(unit);
    h.tick(1);
    assert!(!h.has_stun(unit));
    assert!(h.rigs_of(unit, CcKind::Stun)[0].1.is_some());
}

#[test]
fn root_restores_on_death_with_aura_still_present() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, frost_root());
    h.tick(2);
    assert!(h.has_root(unit));

    // `process_aura_breaks` skips the dead, so the aura outlives its victim. A
    // visual keyed purely on aura presence would ride the corpse.
    h.kill(unit);
    h.tick(1);
    assert!(!h.has_root(unit), "death must count as an exit path");
    assert!(
        h.app.world().get::<ActiveAuras>(unit).is_some(),
        "probe is only meaningful while the aura is still on the corpse"
    );
}

#[test]
fn stun_restores_on_death_with_aura_still_present() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, stun_aura());
    h.tick(2);
    assert!(h.has_stun(unit));

    h.kill(unit);
    h.tick(1);
    assert!(!h.has_stun(unit));
    assert!(h.app.world().get::<ActiveAuras>(unit).is_some());
}

#[test]
fn rigs_despawn_after_retract() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, frost_root());
    h.apply(unit, stun_aura());
    h.tick(3);
    let mid = h.rigs().len();
    assert!(mid > 0, "guard: the drain check must not pass vacuously");

    h.clear_auras(unit);
    h.tick(12);
    assert_eq!(h.rigs().len(), 0, "both rigs drained after their retract");
}

// ==============================================================================
// Composition and scoping
// ==============================================================================

#[test]
fn root_and_stun_compose() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, frost_root());
    h.apply(unit, stun_aura());
    h.tick(2);

    assert!(h.has_root(unit) && h.has_stun(unit), "both must show at once");
    assert_eq!(h.rigs_of(unit, CcKind::Root).len(), 1);
    assert_eq!(h.rigs_of(unit, CcKind::Stun).len(), 1);

    // Drop only the Root. The stun rig must be untouched.
    let remaining = stun_aura();
    h.remove_auras(unit);
    h.apply(unit, remaining);
    h.tick(1);

    assert!(!h.has_root(unit));
    assert!(h.has_stun(unit), "stun survives the root ending");
    assert!(h.rigs_of(unit, CcKind::Root)[0].1.is_some(), "root retracting");
    assert!(
        h.rigs_of(unit, CcKind::Stun)[0].1.is_none(),
        "stun rig must NOT be armed by the root's exit"
    );
}

#[test]
fn restore_is_owner_scoped() {
    let mut h = Harness::new();
    let a = h.spawn_unit(0, 0);
    let b = h.spawn_unit(1, 0);
    h.apply(a, frost_root());
    h.apply(b, frost_root());
    h.tick(2);
    assert_eq!(h.rigs().len(), 2);

    h.remove_auras(a);
    h.tick(1);

    assert!(!h.has_root(a));
    assert!(h.has_root(b), "the other unit is still rooted");
    assert!(h.rigs_of(a, CcKind::Root)[0].1.is_some());
    assert!(
        h.rigs_of(b, CcKind::Root)[0].1.is_none(),
        "a global sweep would have stripped the second unit"
    );
}

#[test]
fn style_change_rebuilds_the_rig() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, web_root());
    h.tick(2);
    assert_eq!(h.root_style_of(unit), Some(RootStyle::Web));

    // A CC replacement swaps the aura within one tick.
    h.remove_auras(unit);
    h.apply(unit, frost_root());
    h.tick(1);

    assert_eq!(
        h.root_style_of(unit),
        Some(RootStyle::Ice),
        "marker must follow the new school"
    );
    let rigs = h.rigs_of(unit, CcKind::Root);
    assert_eq!(rigs.len(), 2, "old rig retracting, new one held");
    assert_eq!(
        rigs.iter().filter(|(_, r)| r.is_none()).count(),
        1,
        "exactly one held rig"
    );
}

#[test]
fn rigs_drain_when_owner_despawns() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, frost_root());
    h.tick(2);
    assert!(!h.rigs().is_empty());

    h.app.world_mut().entity_mut(unit).despawn();
    h.tick(2);
    assert_eq!(h.rigs().len(), 0, "orphaned rigs must not leak");
}

#[test]
fn a_rig_despawned_out_from_under_the_marker_is_rebuilt() {
    // The animation sandbox's `clear_body_state` leftover sweep despawns every
    // `PlayMatchEntity` that is not a `SandboxEntity` — which matches the rig
    // hub — while leaving `StunnedVisual` on the unit. Gating the spawn on the
    // marker's absence made that desync permanent for the rest of the CC.
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, stun_aura());
    h.tick(2);
    let rig = h.rigs_of(unit, CcKind::Stun)[0].0;

    h.app.world_mut().entity_mut(rig).despawn();
    assert!(h.has_stun(unit), "the marker survives the sweep");
    assert_eq!(h.rigs_of(unit, CcKind::Stun).len(), 0);

    h.tick(2);
    assert_eq!(
        h.rigs_of(unit, CcKind::Stun).len(),
        1,
        "the treatment must reconcile, not trust the marker alone"
    );
}

#[test]
fn reconcile_does_not_pop_a_second_flare() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, frost_root());
    h.tick(2);
    // Let the apply flare expire so the count is unambiguous.
    h.tick(20);
    assert_eq!(h.flares(), 0);

    let rig = h.rigs_of(unit, CcKind::Root)[0].0;
    h.app.world_mut().entity_mut(rig).despawn();
    h.tick(2);

    assert_eq!(h.rigs_of(unit, CcKind::Root).len(), 1, "rebuilt");
    assert_eq!(
        h.flares(),
        0,
        "a silent reconcile must not re-announce the landing"
    );
}

#[test]
fn the_whirl_and_flare_actually_emit() {
    // Bevy's `pbr.wgsl` unlit branch is `out.color = material.base_color`, which
    // DISCARDS emissive — it is only added inside `apply_pbr_lighting`. An unlit
    // bead is LDR white with nothing for `Bloom::NATURAL` to bloom, so the whirl
    // renders as a flat ring of dots instead of a shining one. This shipped
    // once; it must not ship again.
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, stun_aura());
    h.tick(2);

    let rig = h.rigs_of(unit, CcKind::Stun)[0].0;
    let children: Vec<Entity> = h.app.world().get::<Children>(rig).unwrap().iter().collect();
    assert_eq!(children.len(), 10);

    let mut emitting = 0;
    for child in &children {
        let handle = h
            .app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(*child)
            .expect("every bead has a material")
            .0
            .clone();
        let materials = h.app.world().resource::<Assets<StandardMaterial>>();
        let m = materials.get(&handle).unwrap();
        assert!(!m.unlit, "an unlit bead silently loses its emissive");
        assert_eq!(m.alpha_mode, AlphaMode::Add, "beads composite additively");
        // The soft edge lives in the texture's alpha — geometry cannot make
        // one, and an untextured additive quad is a hard-edged square.
        assert!(
            m.base_color_texture.is_some() && m.emissive_texture.is_some(),
            "a sparkle needs its texture on BOTH channels"
        );
        if m.emissive.red + m.emissive.green + m.emissive.blue > 1.0 {
            emitting += 1;
        }
    }
    assert_eq!(emitting, 10, "every sparkle must emit above LDR");

    // The flare is the brightest single moment in the treatment.
    let flare = h
        .app
        .world_mut()
        .query_filtered::<Entity, With<CcFlare>>()
        .iter(h.app.world())
        .next()
        .expect("apply flare");
    let handle = h
        .app
        .world()
        .get::<MeshMaterial3d<StandardMaterial>>(flare)
        .unwrap()
        .0
        .clone();
    let materials = h.app.world().resource::<Assets<StandardMaterial>>();
    let m = materials.get(&handle).unwrap();
    assert!(!m.unlit, "an unlit flare silently loses its emissive");
    assert!(m.emissive.blue > 1.0, "the flare must be HDR-overbright");
}

// ==============================================================================
// Motion invariants
// ==============================================================================

#[test]
fn whirl_spins_on_the_wall_clock_over_a_stationary_unit() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    let start = h.app.world().get::<Transform>(unit).unwrap().translation;
    h.apply(unit, stun_aura());
    h.tick(1);
    let rig = h.rigs_of(unit, CcKind::Stun)[0].0;

    // Sample a full second of ticks. The unit never moves, so a spin gated on
    // sim displacement would sit frozen — the strobe bug this guards.
    let mut previous = h.rig_rotation(rig).to_euler(EulerRot::YXZ).0;
    let mut total = 0.0_f32;
    for _ in 0..20 {
        h.tick(1);
        let now = h.rig_rotation(rig).to_euler(EulerRot::YXZ).0;
        let mut delta = now - previous;
        // Unwrap across the +/-PI seam.
        while delta > std::f32::consts::PI {
            delta -= std::f32::consts::TAU;
        }
        while delta < -std::f32::consts::PI {
            delta += std::f32::consts::TAU;
        }
        total += delta.abs();
        previous = now;
    }

    let end = h.app.world().get::<Transform>(unit).unwrap().translation;
    assert_eq!(start, end, "the victim never moved");
    // STUN_SPIN_HZ = 1.0 over 20 * 50ms = 1.0s, within one tick of slack.
    assert!(
        (total - std::f32::consts::TAU).abs() < 0.4,
        "expected ~1 revolution per second, got {total} rad"
    );
}

#[test]
fn whirl_anchors_off_the_rendered_body_not_the_sim_entity() {
    let mut h = Harness::new();
    let owner = h.spawn_unit(0, 0);

    // Combatant: body centre 1.0 (rest_y 0.0), crown 2.25, whirl 2.55.
    h.apply(owner, stun_aura());
    h.tick(2);
    let combatant_rig = h.rigs_of(owner, CcKind::Stun)[0].0;
    let y = h.rig_y(combatant_rig);
    assert!(
        (y - 2.55).abs() < 0.05,
        "combatant whirl should sit at ~2.55, got {y}"
    );

    // Pet: sim y 1.75 but body centre 0.3, crown 0.95, whirl 1.20. Anchoring off
    // the sim y would give 2.20 — nearly two yards above its head.
    let pet = h.spawn_pet(owner);
    h.apply(pet, stun_aura());
    h.tick(2);
    let pet_rig = h.rigs_of(pet, CcKind::Stun)[0].0;
    let py = h.rig_y(pet_rig);
    assert!(
        (py - 1.20).abs() < 0.05,
        "pet whirl should sit at ~1.20 (just over its 0.95 crown), got {py}"
    );
    assert!(py < 1.6, "the whirl must not float above the pet");
}

#[test]
fn retract_finishes_within_the_boar_charge_floor() {
    // Boar Charge is a 1.5s stun; at the third DR application it is 0.25x =
    // 0.375s. Grow plus retract must resolve inside that or the shortest real
    // stun in the game is one flicker.
    let grow_plus_retract = 0.14 + 0.18;
    assert!(
        grow_plus_retract < 0.375,
        "stun envelope must fit the shortest possible stun"
    );

    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, stun_aura());
    // ~0.375s held.
    h.tick(8);
    assert!(h.has_stun(unit));
    h.clear_auras(unit);
    h.tick(12);
    assert_eq!(h.rigs().len(), 0, "fully resolved well inside 1s");
}

#[test]
fn root_broken_mid_grow_retracts_from_where_it_got_to() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, frost_root());
    // Two ticks: the rig is SPAWNED at zero scale on tick 1 (a deferred
    // Command, so `update_cc_rigs` cannot see it until the flush — it is
    // invisible rather than mispositioned for that frame), then driven on tick
    // 2, putting it 50ms into a 180ms grow and partway up.
    h.tick(2);
    let rig = h.rigs_of(unit, CcKind::Root)[0].0;
    let partial = h.rig_scale(rig);
    assert!(partial > 0.0 && partial < 1.0, "mid-grow, got {partial}");

    h.clear_auras(unit);
    h.tick(1);
    assert!(
        h.rig_scale(rig) <= partial,
        "a broken root must sink from its partial height, never pop to full"
    );
}

// ==============================================================================
// Flares
// ==============================================================================

#[test]
fn flares_expire_without_leaking() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);
    h.apply(unit, frost_root());
    h.tick(1);
    assert!(h.flares() > 0, "guard: the drain check must not pass vacuously");

    // CC_FLARE_SECS is 0.40; 20 ticks is 1.0s.
    h.tick(20);
    assert_eq!(h.flares(), 0, "flares must not leak");
}

// ==============================================================================
// Pure seams — no App
// ==============================================================================

#[test]
fn root_style_maps_school_to_object() {
    let mut nature = frost_root();
    nature.spell_school = Some(SpellSchool::Nature);
    assert_eq!(root_style(&nature), RootStyle::Web);

    assert_eq!(root_style(&frost_root()), RootStyle::Ice);

    let mut schoolless = frost_root();
    schoolless.spell_school = None;
    assert_eq!(
        root_style(&schoolless),
        RootStyle::Ice,
        "an aura that lost its school falls back to ice"
    );

    let mut shadow = frost_root();
    shadow.spell_school = Some(SpellSchool::Shadow);
    assert_eq!(root_style(&shadow), RootStyle::Ice);
}

#[test]
fn cc_envelope_is_monotone_and_bounded() {
    let grow = 0.18;
    let retract = 0.22;

    assert_eq!(cc_envelope(0.0, None, grow, retract), 0.0);
    assert_eq!(cc_envelope(grow, None, grow, retract), 1.0);
    assert_eq!(cc_envelope(grow * 4.0, None, grow, retract), 1.0, "clamped");

    // Monotone non-decreasing while growing.
    let mut previous = 0.0;
    for i in 0..=20 {
        let v = cc_envelope(grow * i as f32 / 10.0, None, grow, retract);
        assert!(v >= previous, "grow must not dip");
        previous = v;
    }

    // Armed mid-grow at age 0.05: the grow term must FREEZE there, so the
    // envelope only ever falls from that height. `age` keeps advancing while
    // retracting, which is why each sample is `armed_age + r`.
    let armed_age = 0.05;
    let armed_at = cc_envelope(armed_age, None, grow, retract);
    let mut previous = armed_at;
    for i in 0..=10 {
        let r = retract * i as f32 / 10.0;
        let v = cc_envelope(armed_age + r, Some(r), grow, retract);
        assert!(
            v <= armed_at + f32::EPSILON,
            "a CC broken mid-grow must never rise above the height it had reached"
        );
        assert!(v <= previous + f32::EPSILON, "retract must be monotone");
        previous = v;
    }
    assert_eq!(
        cc_envelope(armed_age + retract, Some(retract), grow, retract),
        0.0
    );
    assert_eq!(cc_envelope(10.0, Some(retract * 2.0), grow, retract), 0.0);
}

// ==============================================================================
// Frost Nova propagation
// ==============================================================================

/// The apply flare must wait for the wavefront, like the rig it announces.
///
/// Regression: the flare took no delay at all, so a nova catching victims at
/// different distances popped every "landing" ring at t=0 and then raised their
/// crystals seconds apart — the flare contradicting the propagation it exists to
/// announce. Asserts the RENDERED scale, not the stored field.
#[test]
fn a_delayed_root_flare_stays_invisible_until_the_wave_arrives() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(0, 0);

    // A victim at the far edge of the nova: the wave takes most of its life to
    // get there.
    h.app
        .world_mut()
        .entity_mut(unit)
        .insert(NovaFreezeDelay { secs: 0.5, age: 0.0 });
    h.apply(unit, frost_root());
    h.tick(1);

    let flare = h
        .app
        .world_mut()
        .query_filtered::<Entity, With<CcFlare>>()
        .iter(h.app.world())
        .next()
        .expect("a root still spawns its flare immediately");
    assert!(
        (h.app.world().get::<CcFlare>(flare).unwrap().delay - 0.5).abs() < 1e-3,
        "the flare must inherit the rig's wavefront delay"
    );

    // Mid-delay: spawned, but drawing nothing.
    h.tick(4);
    let scale = h.app.world().get::<Transform>(flare).unwrap().scale;
    assert_eq!(
        scale,
        Vec3::ZERO,
        "the flare rendered before the wavefront reached the victim"
    );

    // Past the delay it expands normally, and gets its FULL span — the delay
    // holds the clock rather than eating into the flare's life.
    h.tick(8);
    let scale = h.app.world().get::<Transform>(flare).unwrap().scale;
    assert!(
        scale.x > 0.0,
        "the flare never started after its delay elapsed"
    );
}

/// The outer ring is a promise about where the freeze reaches, so it must land
/// on the gameplay radius rather than a constant that can drift from it.
#[test]
fn the_wavefront_lands_on_the_gameplay_radius() {
    let defs = AbilityDefinitions::default();
    let range = defs
        .get(&AbilityType::FrostNova)
        .expect("FrostNova is defined in abilities.ron")
        .range;
    assert!(
        (nova_outer_radius() - range).abs() < 1e-3,
        "the wavefront stops at {}yd but Frost Nova roots out to {}yd — victims \
         between the two freeze instantly, outside a wave that never arrives",
        nova_outer_radius(),
        range
    );
}
