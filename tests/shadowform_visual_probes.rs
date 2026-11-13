//! Probes for the Shadowform look (`rendering/effects/shadowform.rs`): the
//! violet, translucent body and the feet shroud.
//!
//! The body edits go IN PLACE on the body's own material, never through the
//! shared `OriginalBodyMaterial` slot Fear and Polymorph use. What that has to
//! buy is checked here in both orders: a Fear's husk and a Polymorph's wool,
//! landing after the form or under it, each give the look back when they lift.
//! The lift paths (the aura gone, the holder dead), the stealth visual leaving
//! the look alone, and the shroud's life (spawned with the look, riding its
//! Priest's feet, retiring its particles, owner-scoped) are the rest.
//!
//! The systems run in the order `states/mod.rs` registers them: stealth, then
//! the look at the head of the Polymorph / Fear chain.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` — no window, no GPU.

use std::time::Duration;

use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraType, ClientParticle, Combatant, CompoundDebuff, OriginalBodyMaterial,
    OriginalMesh, ShadowformLook, ShadowformShroud, VisualBody,
};
use arenasim::states::play_match::{
    animate_client_particles, billboard_shadowform_shrouds, shadowform_body_color,
    update_fear_visuals, update_polymorph_visuals, update_shadowform_shrouds,
    update_shadowform_visuals, update_stealth_visuals,
};
use arenasim::CharacterClass;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

const TICK: Duration = Duration::from_millis(100);

/// The Priest's shipped body colour (`class_mesh_color`).
const PRIEST_WHITE: Color = Color::srgb(0.95, 0.95, 0.95);

/// The form's face, as `apply_pending_auras` lands it.
fn shadowform_aura() -> Aura {
    Aura {
        effect_type: AuraType::DamageDealtIncrease,
        duration: 600.0,
        magnitude: 0.15,
        ability_name: "Shadowform".into(),
        compound: Some(CompoundDebuff::Shadowform),
        ..Default::default()
    }
}

/// The same aura type with no compound: a buff that is not the form.
fn other_damage_buff() -> Aura {
    Aura {
        effect_type: AuraType::DamageDealtIncrease,
        duration: 15.0,
        magnitude: 0.2,
        ability_name: "Power Infusion".into(),
        ..Default::default()
    }
}

fn fear_aura() -> Aura {
    Aura {
        effect_type: AuraType::Fear,
        duration: 8.0,
        break_on_damage_threshold: 0.0,
        ..Default::default()
    }
}

fn polymorph_aura() -> Aura {
    Aura {
        effect_type: AuraType::Polymorph,
        duration: 8.0,
        break_on_damage_threshold: 0.0,
        ..Default::default()
    }
}

struct Unit {
    unit: Entity,
    body: Entity,
    material: Handle<StandardMaterial>,
    mesh: Handle<Mesh>,
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
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
        app.add_systems(
            Update,
            (
                update_stealth_visuals,
                update_shadowform_visuals,
                update_polymorph_visuals,
                update_fear_visuals,
                update_shadowform_shrouds,
                animate_client_particles,
                billboard_shadowform_shrouds,
            )
                .chain(),
        );
        Harness { app }
    }

    /// A Priest at `x`, its body as `spawn_combatant` builds it: its own
    /// material, white, alpha-blended.
    fn spawn_priest(&mut self, team: u8, x: f32) -> Unit {
        let world = self.app.world_mut();
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(Capsule3d::new(0.5, 1.5));
        let material = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: PRIEST_WHITE,
                alpha_mode: AlphaMode::Blend,
                ..default()
            });
        let body = world
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                OriginalMesh(mesh.clone()),
                VisualBody { rest_y: 0.0 },
                Transform::default(),
            ))
            .id();
        let unit = world
            .spawn((
                Transform::from_xyz(x, 1.0, 0.0),
                Combatant::new(team, 0, CharacterClass::Priest),
            ))
            .id();
        world.entity_mut(unit).add_child(body);
        Unit {
            unit,
            body,
            material,
            mesh,
        }
    }

    fn set_auras(&mut self, unit: Entity, auras: Vec<Aura>) {
        self.app
            .world_mut()
            .entity_mut(unit)
            .insert(ActiveAuras { auras });
    }

    fn tick(&mut self) {
        self.app.update();
    }

    fn worn_material(&self, body: Entity) -> Handle<StandardMaterial> {
        self.app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(body)
            .unwrap()
            .0
            .clone()
    }

    fn worn_mesh(&self, body: Entity) -> Handle<Mesh> {
        self.app.world().get::<Mesh3d>(body).unwrap().0.clone()
    }

    fn material(&self, handle: &Handle<StandardMaterial>) -> StandardMaterial {
        self.app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(handle)
            .unwrap()
            .clone()
    }

    fn look_on(&self, body: Entity) -> bool {
        self.app.world().get::<ShadowformLook>(body).is_some()
    }

    fn shrouds_of(&mut self, owner: Entity) -> Vec<Entity> {
        self.app
            .world_mut()
            .query::<(Entity, &ShadowformShroud)>()
            .iter(self.app.world())
            .filter(|(_, s)| s.owner == owner)
            .map(|(e, _)| e)
            .collect()
    }

    fn shroud_particles(&mut self, rig: Entity) -> usize {
        let Some(children) = self.app.world().get::<Children>(rig) else {
            return 0;
        };
        let children: Vec<Entity> = children.iter().collect();
        children
            .into_iter()
            .filter(|c| self.app.world().get::<ClientParticle>(*c).is_some())
            .count()
    }

    /// Asserts the body's own material wears the form's look, and the body
    /// wears that material.
    fn assert_wears_look(&self, u: &Unit, what: &str) {
        assert_eq!(
            self.worn_material(u.body),
            u.material,
            "{what}: own material worn"
        );
        let m = self.material(&u.material);
        assert_eq!(
            m.base_color,
            shadowform_body_color(PRIEST_WHITE),
            "{what}: tinted"
        );
        assert_eq!(m.base_color.alpha(), 0.65, "{what}: translucent");
        assert_eq!(m.alpha_mode, AlphaMode::Blend, "{what}: blended");
        assert!(self.look_on(u.body), "{what}: look marked");
    }

    /// Asserts the body's own material is back as it was spawned.
    fn assert_untouched(&self, u: &Unit, what: &str) {
        let m = self.material(&u.material);
        assert_eq!(m.base_color, PRIEST_WHITE, "{what}: colour");
        assert_eq!(m.alpha_mode, AlphaMode::Blend, "{what}: alpha mode");
        assert!(!self.look_on(u.body), "{what}: look unmarked");
    }
}

/// The form lands: the body keeps its own material HANDLE (no swap, nothing
/// in the shared restore slot), that material is tinted and translucent, and
/// one shroud rig spawns. A Priest holding a same-type buff that is not the
/// form, ticked alongside, is untouched — the look keys on the compound.
#[test]
fn the_form_tints_the_body_in_place() {
    let mut h = Harness::new();
    let priest = h.spawn_priest(1, 0.0);
    let control = h.spawn_priest(2, 6.0);

    h.set_auras(priest.unit, vec![shadowform_aura()]);
    h.set_auras(control.unit, vec![other_damage_buff()]);
    h.tick();

    h.assert_wears_look(&priest, "in Shadowform");
    assert!(
        h.app
            .world()
            .get::<OriginalBodyMaterial>(priest.body)
            .is_none(),
        "the shared restore slot is never used"
    );
    assert_eq!(h.shrouds_of(priest.unit).len(), 1, "one shroud");

    assert_eq!(h.worn_material(control.body), control.material);
    h.assert_untouched(&control, "another damage buff");
    assert!(h.shrouds_of(control.unit).is_empty(), "no shroud");

    // Held: nothing re-applies or doubles over further ticks.
    for _ in 0..5 {
        h.tick();
    }
    h.assert_wears_look(&priest, "held");
    assert_eq!(h.shrouds_of(priest.unit).len(), 1, "still one shroud");
}

/// The look records what it changed and puts back exactly that: a body whose
/// material was opaque is blended while the form holds (or its alpha would not
/// show) and opaque again after.
#[test]
fn the_look_puts_back_the_alpha_mode_it_found() {
    let mut h = Harness::new();
    let priest = h.spawn_priest(1, 0.0);
    h.app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .get_mut(&priest.material)
        .unwrap()
        .alpha_mode = AlphaMode::Opaque;
    h.set_auras(priest.unit, vec![shadowform_aura()]);
    h.tick();
    assert_eq!(h.material(&priest.material).alpha_mode, AlphaMode::Blend);

    h.set_auras(priest.unit, vec![]);
    h.tick();
    let m = h.material(&priest.material);
    assert_eq!(m.alpha_mode, AlphaMode::Opaque, "opaque again");
    assert_eq!(m.base_color, PRIEST_WHITE, "untinted");
}

/// The aura gone — the component removed outright, or the vec emptied — puts
/// the material back exactly and takes the shroud away.
#[test]
fn the_look_lifts_when_the_form_ends() {
    for removed_outright in [true, false] {
        let mut h = Harness::new();
        let priest = h.spawn_priest(1, 0.0);
        h.set_auras(priest.unit, vec![shadowform_aura()]);
        h.tick();
        h.assert_wears_look(&priest, "in Shadowform");

        if removed_outright {
            h.app
                .world_mut()
                .entity_mut(priest.unit)
                .remove::<ActiveAuras>();
        } else {
            h.set_auras(priest.unit, vec![]);
        }
        h.tick();
        h.assert_untouched(&priest, "form ended");
        assert!(h.shrouds_of(priest.unit).is_empty(), "shroud gone");
    }
}

/// A killing blow leaves the aura on the corpse, so death is a lift too.
#[test]
fn the_look_lifts_on_death() {
    let mut h = Harness::new();
    let priest = h.spawn_priest(1, 0.0);
    h.set_auras(priest.unit, vec![shadowform_aura()]);
    h.tick();
    h.assert_wears_look(&priest, "alive");

    h.app
        .world_mut()
        .get_mut::<Combatant>(priest.unit)
        .unwrap()
        .current_health = 0.0;
    h.tick();
    h.assert_untouched(&priest, "dead");
    assert!(h.shrouds_of(priest.unit).is_empty(), "shroud gone");
}

/// Fear AFTER the form: the husk swaps the handle (the look is not on the
/// husk), and Fear's restore hands back the tinted material.
#[test]
fn a_fear_after_the_form_gives_the_look_back() {
    let mut h = Harness::new();
    let priest = h.spawn_priest(1, 0.0);
    h.set_auras(priest.unit, vec![shadowform_aura()]);
    h.tick();

    h.set_auras(priest.unit, vec![shadowform_aura(), fear_aura()]);
    h.tick();
    let husk = h.worn_material(priest.body);
    assert_ne!(husk, priest.material, "the husk is worn");
    assert_eq!(
        h.material(&husk).base_color.alpha(),
        1.0,
        "the husk is not tinted"
    );

    h.set_auras(priest.unit, vec![shadowform_aura()]);
    h.tick();
    h.assert_wears_look(&priest, "after the fear");
    assert_eq!(h.shrouds_of(priest.unit).len(), 1, "one shroud throughout");
}

/// The form landing UNDER a Fear: the look goes on the body's true material,
/// in the shared slot, never on the husk, and is worn once the fear lifts.
#[test]
fn the_form_under_a_fear_lands_on_the_true_material() {
    let mut h = Harness::new();
    let priest = h.spawn_priest(1, 0.0);
    h.set_auras(priest.unit, vec![fear_aura()]);
    h.tick();
    let husk = h.worn_material(priest.body);
    let husk_colour = h.material(&husk).base_color;
    assert_ne!(husk, priest.material, "the husk is worn");

    h.set_auras(priest.unit, vec![fear_aura(), shadowform_aura()]);
    h.tick();
    assert_eq!(h.worn_material(priest.body), husk, "still the husk");
    assert_eq!(h.material(&husk).base_color, husk_colour, "husk untouched");
    assert_eq!(
        h.material(&priest.material).base_color,
        shadowform_body_color(PRIEST_WHITE),
        "the true material is tinted"
    );

    h.set_auras(priest.unit, vec![shadowform_aura()]);
    h.tick();
    h.assert_wears_look(&priest, "after the fear");
}

/// Polymorph AFTER the form: the wool and the sheep's mesh swap in, and the
/// restore brings back both the capsule and the tinted material.
#[test]
fn a_polymorph_after_the_form_gives_the_look_back() {
    let mut h = Harness::new();
    let priest = h.spawn_priest(1, 0.0);
    h.set_auras(priest.unit, vec![shadowform_aura()]);
    h.tick();

    h.set_auras(priest.unit, vec![shadowform_aura(), polymorph_aura()]);
    h.tick();
    let wool = h.worn_material(priest.body);
    assert_ne!(wool, priest.material, "the wool is worn");
    assert_ne!(h.worn_mesh(priest.body), priest.mesh, "the sheep is worn");
    assert_eq!(
        h.material(&wool).base_color.alpha(),
        1.0,
        "the wool is not tinted"
    );

    h.set_auras(priest.unit, vec![shadowform_aura()]);
    h.tick();
    assert_eq!(h.worn_mesh(priest.body), priest.mesh, "the capsule is back");
    h.assert_wears_look(&priest, "after the polymorph");
}

/// The form landing UNDER a Polymorph: the look goes on the true material,
/// never on the wool, and both the capsule and the look come back.
#[test]
fn the_form_under_a_polymorph_lands_on_the_true_material() {
    let mut h = Harness::new();
    let priest = h.spawn_priest(1, 0.0);
    h.set_auras(priest.unit, vec![polymorph_aura()]);
    h.tick();
    let wool = h.worn_material(priest.body);
    let wool_colour = h.material(&wool).base_color;

    h.set_auras(priest.unit, vec![polymorph_aura(), shadowform_aura()]);
    h.tick();
    assert_eq!(h.worn_material(priest.body), wool, "still the wool");
    assert_eq!(h.material(&wool).base_color, wool_colour, "wool untouched");

    h.set_auras(priest.unit, vec![shadowform_aura()]);
    h.tick();
    assert_eq!(h.worn_mesh(priest.body), priest.mesh, "the capsule is back");
    h.assert_wears_look(&priest, "after the polymorph");
}

/// The stealth visual runs on every change to the combatant and reads a
/// translucent, unstealthed body as its own fade to "restore". It must leave a
/// body wearing the look alone.
#[test]
fn the_stealth_visual_leaves_the_look_alone() {
    let mut h = Harness::new();
    let priest = h.spawn_priest(1, 0.0);
    h.set_auras(priest.unit, vec![shadowform_aura()]);
    h.tick();
    for _ in 0..5 {
        h.app
            .world_mut()
            .get_mut::<Combatant>(priest.unit)
            .unwrap()
            .set_changed();
        h.tick();
        h.assert_wears_look(&priest, "the combatant changed");
    }
}

/// How the two would compose, were a unit in Shadowform ever stealthed: the
/// form's look holds while both do, and once the form ends and puts its
/// recorded values back, stealth's next change takes the body to its fade.
#[test]
fn stealth_takes_the_body_back_once_the_form_ends() {
    let mut h = Harness::new();
    let priest = h.spawn_priest(1, 0.0);
    h.set_auras(priest.unit, vec![shadowform_aura()]);
    h.tick();
    h.app
        .world_mut()
        .get_mut::<Combatant>(priest.unit)
        .unwrap()
        .stealthed = true;
    h.tick();
    h.assert_wears_look(&priest, "stealthed in the form");

    h.set_auras(priest.unit, vec![]);
    h.tick();
    h.app
        .world_mut()
        .get_mut::<Combatant>(priest.unit)
        .unwrap()
        .set_changed();
    h.tick();
    let faded = h.material(&priest.material).base_color.to_srgba();
    assert_eq!(faded.alpha, 0.4, "stealth's fade");
    assert!(
        (faded.red - 0.95 * 0.6).abs() < 1e-5,
        "stealth's darkening of the untinted body"
    );
}

/// The shroud rides its Priest's feet and emits its three layers at the
/// bench's rates (50 + 50 + 1.1 a second), retiring each particle at the end
/// of its life so a held form keeps a steady count rather than a pile.
#[test]
fn the_shroud_follows_the_feet_and_keeps_a_steady_count() {
    let mut h = Harness::new();
    let priest = h.spawn_priest(1, 0.0);
    h.set_auras(priest.unit, vec![shadowform_aura()]);
    h.tick();
    let rig = h.shrouds_of(priest.unit)[0];

    h.app
        .world_mut()
        .get_mut::<Transform>(priest.unit)
        .unwrap()
        .translation = Vec3::new(5.0, 1.0, -3.0);
    // 2 s at 10 ticks a second.
    for _ in 0..20 {
        h.tick();
    }
    let at = h.app.world().get::<Transform>(rig).unwrap().translation;
    assert_eq!((at.x, at.z), (5.0, -3.0), "the shroud rides the Priest");
    assert!(at.y < 0.1, "at the feet: {}", at.y);

    // The flames and flares live 1.0 s and the smoke 3.0 s: in a steady state
    // about 100 + 3 are alive. Emission and retirement are per tick, so allow
    // a tick of each on top.
    for _ in 0..30 {
        h.tick();
    }
    let alive = h.shroud_particles(rig);
    assert!(
        (95..=125).contains(&alive),
        "a steady ~103 particles, got {alive}"
    );
}

/// Two Priests in the form each keep their own shroud, and one Priest's form
/// ending takes only its own. A shroud whose Priest is gone goes too.
#[test]
fn the_shroud_is_owner_scoped() {
    let mut h = Harness::new();
    let a = h.spawn_priest(1, 0.0);
    let b = h.spawn_priest(2, 6.0);
    h.set_auras(a.unit, vec![shadowform_aura()]);
    h.set_auras(b.unit, vec![shadowform_aura()]);
    h.tick();
    assert_eq!(h.shrouds_of(a.unit).len(), 1);
    assert_eq!(h.shrouds_of(b.unit).len(), 1);

    h.set_auras(a.unit, vec![]);
    h.tick();
    assert!(h.shrouds_of(a.unit).is_empty(), "a's shroud gone");
    assert_eq!(h.shrouds_of(b.unit).len(), 1, "b's shroud stays");
    h.assert_wears_look(&b, "b still in the form");

    h.app.world_mut().entity_mut(b.unit).despawn();
    h.tick();
    let left = h
        .app
        .world_mut()
        .query::<&ShadowformShroud>()
        .iter(h.app.world())
        .count();
    assert_eq!(left, 0, "a shroud without its Priest goes");
}

/// Every live shroud particle faces the camera, wherever the camera stands:
/// the rig is unrotated, so each particle's rotation is the camera's own.
#[test]
fn the_shroud_particles_face_the_camera() {
    for cam_at in [Vec3::new(12.0, 8.0, 0.0), Vec3::new(-3.0, 6.0, 14.0)] {
        let mut h = Harness::new();
        let cam = Transform::from_translation(cam_at).looking_at(Vec3::ZERO, Vec3::Y);
        h.app.world_mut().spawn((Camera3d::default(), cam));
        let priest = h.spawn_priest(1, 0.0);
        h.set_auras(priest.unit, vec![shadowform_aura()]);
        for _ in 0..5 {
            h.tick();
        }
        let rig = h.shrouds_of(priest.unit)[0];
        let children: Vec<Entity> = h
            .app
            .world()
            .get::<Children>(rig)
            .expect("the shroud has particles")
            .iter()
            .collect();
        let mut checked = 0;
        for child in children {
            let world = h.app.world();
            let Some(particle) = world.get::<ClientParticle>(child) else {
                continue;
            };
            if particle.age >= particle.life {
                continue;
            }
            let rot = world.get::<Transform>(child).unwrap().rotation;
            assert!(
                (rot * Vec3::Z).dot(cam.rotation * Vec3::Z) > 0.999,
                "a shroud particle does not face the camera at {cam_at}"
            );
            assert!(
                (rot * Vec3::Y).dot(cam.rotation * Vec3::Y) > 0.999,
                "a shroud particle is rolled against the camera at {cam_at}"
            );
            checked += 1;
        }
        assert!(checked > 20, "too few live particles checked: {checked}");
    }
}
