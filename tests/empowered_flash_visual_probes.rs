//! Probes for the empowered ground flash and Shadow Trance's speck halo
//! (`rendering/effects/empowered_flash.rs`, AS-346).
//!
//! Two halves:
//!
//! - **The sim's marker**, driven through `apply_pending_auras` on a bare
//!   `World`: Amplify Curse's cast and Nightfall's Shadow Trance each mark
//!   their landing on the Warlock, another primed cast marks none, and both
//!   names are the shipped config's.
//! - **The renderer**, on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin`
//!   with the chain `states/mod.rs` registers (and the shared particle ager),
//!   asserting what is DRAWN: the flash's three flat additive discs at the
//!   feet (mesh, texture, colour, alpha, blend, size and their bloom), the
//!   halo's near-black alpha-blended blob specks at the head (count, sprite,
//!   colour, blend, size, inward drift), the specks turned to a ROTATED camera,
//!   and the edges: nothing on a corpse, the flash playing out after the curse
//!   spends its buff, the halo gone the moment Shadow Trance is spent or its
//!   Warlock dies, one halo per Warlock, and both read through the default
//!   match camera.

use std::collections::BTreeSet;
use std::f32::consts::PI;
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::render::camera::CameraProjection;
use bevy::render::mesh::VertexAttributeValues;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::auras::apply_pending_auras;
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraApplyOwner, AuraApplyRoute, AuraLanding,
    AuraLandingKind, AuraPending, AuraType, ClientParticle, Combatant, EmpoweredFlash,
    EmpoweredFlashDisc, GameRng, ShadowTranceHalo, AMPLIFY_CURSE_AURA, SHADOW_TRANCE_AURA,
};
use arenasim::states::play_match::talent_procs::{effect_aura, ProcEvent, TalentProcSlot};
use arenasim::states::play_match::talents::{TalentDefinitions, TalentEffect, TalentId};
use arenasim::states::play_match::{
    animate_client_particles, animate_druid_effects, animate_druid_particles,
    animate_empowered_flashes, billboard_druid_particles, billboard_shadow_trance_specks,
    cleanup_druid_effects, flash_frame, spawn_druid_effects, spawn_empowered_flashes,
    spawn_paladin_oneshots, speck_launch, update_shadow_trance_halos, AbilityDefinitions,
    AbilityType, SHADOW_TRANCE_HALO_ENDS_WITH_PROC,
};
use arenasim::CharacterClass;

// The bench's numbers, as literals: a mutant of the module's constants must
// not carry the expectation along with it.
const AMPLIFY_FLASH_MAX_DIAMETER: f32 = 4.1;
const FLASH_DISC_HALF_WIDTHS: [f32; 3] = [0.63, 0.51, 0.41];
const FLASH_ABOVE_FLOOR: f32 = 0.03;
const FLASH_GLOW: f32 = 2.5;
const BLOB_EDGE: f32 = 0.7;
const SHADOW_TRANCE_SPECK_SIZE: f32 = 0.028;
const SPECK_SHELL_RADIUS: f32 = 0.7;
const TRANCE_HEAD_ABOVE_FLOOR: f32 = 2.25;

// ── the sim's marker ─────────────────────────────────────────────────────────

fn world() -> World {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(GameRng::from_seed(7));
    world.insert_resource(Time::<()>::default());
    world
}

fn landings(world: &mut World) -> Vec<(Entity, AuraLandingKind)> {
    let mut q = world.query::<&AuraLanding>();
    q.iter(world).map(|l| (l.target, l.kind)).collect()
}

fn primed(name: &str) -> Aura {
    Aura {
        effect_type: AuraType::NextCast,
        duration: 10.0,
        magnitude: 1.0,
        ability_name: name.into(),
        ..Default::default()
    }
}

/// Shadow Trance as Nightfall's proc lands it, through the engine's own
/// constructor, from the shipped talent tree.
fn shipped_shadow_trance() -> Aura {
    let defs = TalentDefinitions::shipped();
    let node = defs
        .iter()
        .find(|(id, _)| *id == TalentId::Nightfall)
        .map(|(_, node)| node)
        .expect("Nightfall is a talent");
    let proc = node
        .per_rank
        .iter()
        .find_map(|e| match e {
            TalentEffect::Proc(p) => Some(p.clone()),
            _ => None,
        })
        .expect("Nightfall carries a proc");
    let slot = TalentProcSlot::new(TalentId::Nightfall, &node.name, proc);
    let owner = Entity::from_raw(1);
    let event = ProcEvent::new(owner, slot.proc.trigger)
        .amount(50.0)
        .other(Entity::from_raw(2));
    effect_aura(&slot, owner, &event)
        .expect("Nightfall lands an aura")
        .aura
}

/// The router's names are the shipped config's: a rename in `abilities.ron`
/// or `talents.ron` must not silently drop either landing.
#[test]
fn the_routed_names_are_the_config_names() {
    let defs = AbilityDefinitions::default();
    let amplify = defs.get_unchecked(&AbilityType::AmplifyCurse);
    assert_eq!(amplify.name, AMPLIFY_CURSE_AURA);
    assert_eq!(
        amplify.applies_aura.as_ref().map(|a| a.aura_type),
        Some(AuraType::NextCast)
    );
    let trance = shipped_shadow_trance();
    assert_eq!(trance.ability_name, SHADOW_TRANCE_AURA);
    assert_eq!(trance.effect_type, AuraType::NextCast);
}

#[test]
fn amplify_curse_and_shadow_trance_mark_their_landings_and_another_primed_cast_none() {
    let mut world = world();
    let warlock = world
        .spawn((
            Combatant::new(1, 0, CharacterClass::Warlock),
            Transform::default(),
        ))
        .id();
    // Another primed cast lands no landing of its own.
    world.spawn(AuraPending {
        target: warlock,
        aura: primed("A Primed Cast With No Landing"),
    });
    world.run_system_once(apply_pending_auras).unwrap();
    assert!(landings(&mut world).is_empty());

    let defs = AbilityDefinitions::default();
    let pending = AuraPending::from_ability(
        warlock,
        warlock,
        defs.get_unchecked(&AbilityType::AmplifyCurse),
    )
    .expect("Amplify Curse applies an aura");
    world.spawn(pending);
    world.run_system_once(apply_pending_auras).unwrap();
    assert_eq!(
        landings(&mut world),
        vec![(warlock, AuraLandingKind::AmplifyFlash)]
    );

    world.spawn(AuraPending {
        target: warlock,
        aura: shipped_shadow_trance(),
    });
    world.run_system_once(apply_pending_auras).unwrap();
    let kinds: BTreeSet<String> = landings(&mut world)
        .into_iter()
        .map(|(t, k)| {
            assert_eq!(t, warlock);
            format!("{k:?}")
        })
        .collect();
    assert_eq!(
        kinds,
        BTreeSet::from(["AmplifyFlash".to_string(), "ShadowTrance".to_string()])
    );
}

/// The flash draws their application, so the shared aura band stays off
/// them; any other primed cast keeps it.
#[test]
fn the_flash_owns_both_applications_and_other_primed_casts_keep_the_band() {
    let owned = AuraApplyRoute::Owned(AuraApplyOwner::AuraLanding);
    let route = AuraApplyRoute::for_instance;
    assert_eq!(route(AuraType::NextCast, AMPLIFY_CURSE_AURA), owned);
    assert_eq!(route(AuraType::NextCast, SHADOW_TRANCE_AURA), owned);
    assert_eq!(
        route(AuraType::NextCast, "A Primed Cast With No Landing"),
        AuraApplyRoute::Band
    );
}

// ── the renderer ─────────────────────────────────────────────────────────────

const TICK: Duration = Duration::from_millis(16);
const TICK_SECS: f32 = 0.016;
/// Combatants stand with capsule centres at world y = 1.0 over the floor.
const COMBATANT_Y: f32 = 1.0;

/// The specks' near-black (2, 2, 2).
const SPECK_RGB: [u8; 3] = [2, 2, 2];
const TEAL: [u8; 3] = [38, 97, 91];
const BLUE: [u8; 3] = [0, 97, 255];

struct Harness {
    app: App,
    camera: Entity,
}

impl Harness {
    fn new() -> Self {
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
        // The Druid's chain and the Paladin one-shots consume the same marker
        // type, so they run here too, ahead of this one: they must leave these
        // landings alone.
        app.add_systems(
            Update,
            (
                spawn_druid_effects,
                animate_druid_effects,
                animate_druid_particles,
                billboard_druid_particles,
                cleanup_druid_effects,
                spawn_paladin_oneshots,
            )
                .chain()
                .before(spawn_empowered_flashes),
        );
        // The order `states/mod.rs` registers: spawn -> flash -> halo, the
        // shared particle ager, then the specks turned.
        app.add_systems(
            Update,
            (
                spawn_empowered_flashes,
                animate_empowered_flashes,
                update_shadow_trance_halos,
                animate_client_particles,
                billboard_shadow_trance_specks,
            )
                .chain(),
        );
        // A camera turned well off every axis, so a sprite left unrotated (or
        // turned to a fixed direction) cannot pass for one facing it.
        let camera = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Transform::from_xyz(-23.0, 31.0, 17.0).looking_at(Vec3::ZERO, Vec3::Y),
            ))
            .id();
        // Warm the clock: Bevy's first update has a zero delta.
        app.update();
        Harness { app, camera }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn tick_secs(&mut self, secs: f32) {
        self.tick((secs / TICK_SECS).round() as u32);
    }

    fn warlock(&mut self, at: Vec3, trance: bool) -> Entity {
        let auras = if trance {
            vec![primed(SHADOW_TRANCE_AURA)]
        } else {
            Vec::new()
        };
        self.app
            .world_mut()
            .spawn((
                Combatant::new(1, 0, CharacterClass::Warlock),
                Transform::from_translation(at),
                ActiveAuras { auras },
            ))
            .id()
    }

    fn land(&mut self, target: Entity, kind: AuraLandingKind) {
        // A Warlock's own cast: Amplify Curse is self-cast, and Nightfall
        // lands Shadow Trance with its owner as the caster.
        self.app.world_mut().spawn(AuraLanding {
            target,
            kind,
            caster: Some(target),
        });
    }

    fn kill(&mut self, unit: Entity) {
        let mut c = self.app.world_mut().get_mut::<Combatant>(unit).unwrap();
        c.current_health = 0.0;
        c.is_dead = true;
    }

    fn spend_trance(&mut self, unit: Entity) {
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(unit)
            .unwrap()
            .auras
            .retain(|a| a.ability_name != SHADOW_TRANCE_AURA);
    }

    fn move_to(&mut self, unit: Entity, at: Vec3) {
        self.app
            .world_mut()
            .get_mut::<Transform>(unit)
            .unwrap()
            .translation = at;
    }

    fn flashes(&mut self) -> Vec<Entity> {
        let world = self.app.world_mut();
        world
            .query_filtered::<Entity, With<EmpoweredFlash>>()
            .iter(world)
            .collect()
    }

    fn discs(&mut self) -> Vec<(Entity, f32)> {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &EmpoweredFlashDisc)>()
            .iter(world)
            .map(|(e, d)| (e, d.half_width))
            .collect()
    }

    fn halos(&mut self) -> Vec<(Entity, Entity)> {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &ShadowTranceHalo)>()
            .iter(world)
            .map(|(e, h)| (e, h.owner))
            .collect()
    }

    fn halo_emitted(&mut self) -> u32 {
        let world = self.app.world_mut();
        world
            .query::<&ShadowTranceHalo>()
            .iter(world)
            .map(|h| h.emitted)
            .sum()
    }

    /// Every live speck: entity, world position, age.
    fn specks(&mut self) -> Vec<(Entity, Vec3, f32)> {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &ClientParticle, &GlobalTransform)>()
            .iter(world)
            .filter(|(_, p, _)| p.age < p.life)
            .map(|(e, p, g)| (e, g.translation(), p.age))
            .collect()
    }

    fn particle_count(&mut self) -> usize {
        let world = self.app.world_mut();
        world.query::<&ClientParticle>().iter(world).count()
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

    /// The part's mesh's vertices, in its own frame.
    fn local_vertices(&self, part: Entity) -> Vec<Vec3> {
        let world = self.app.world();
        let mesh = world
            .get::<Mesh3d>(part)
            .expect("part has a mesh")
            .0
            .clone();
        let meshes = world.resource::<Assets<Mesh>>();
        let Some(VertexAttributeValues::Float32x3(ps)) = meshes
            .get(&mesh)
            .unwrap()
            .attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        ps.iter().map(|p| Vec3::from(*p)).collect()
    }

    /// The alpha of `texture` along its radius, at fractions `rs` of the
    /// half-width from the centre (to the right of centre, on the middle row).
    fn texture_alpha(&self, texture: &Handle<Image>, rs: &[f32]) -> Vec<f32> {
        let image = self
            .app
            .world()
            .resource::<Assets<Image>>()
            .get(texture)
            .expect("texture");
        let (w, h) = (image.width() as usize, image.height() as usize);
        let data = image.data.as_ref().expect("texels");
        rs.iter()
            .map(|r| {
                let x = ((0.5 + 0.5 * r) * w as f32).min(w as f32 - 1.0) as usize;
                let y = h / 2;
                data[(y * w + x) * 4 + 3] as f32 / 255.0
            })
            .collect()
    }
}

fn feet(at: Vec3) -> Vec3 {
    at - Vec3::Y * COMBATANT_Y
}

fn head(at: Vec3) -> Vec3 {
    feet(at) + Vec3::Y * TRANCE_HEAD_ABOVE_FLOOR
}

fn srgb(c: Color) -> [f32; 3] {
    let s = c.to_srgba();
    [s.red * 255.0, s.green * 255.0, s.blue * 255.0]
}

fn assert_rgb(c: Color, want: [u8; 3], tol: f32, what: &str) {
    let got = srgb(c);
    for k in 0..3 {
        assert!(
            (got[k] - want[k] as f32).abs() <= tol,
            "{what}: rgb {got:?}, want {want:?}"
        );
    }
}

#[test]
fn the_flash_is_three_flat_additive_discs_on_the_floor_at_the_feet() {
    let mut h = Harness::new();
    let at = Vec3::new(4.0, COMBATANT_Y, -3.0);
    let warlock = h.warlock(at, false);
    h.land(warlock, AuraLandingKind::AmplifyFlash);
    h.tick(1);

    assert_eq!(h.flashes().len(), 1);
    let discs = h.discs();
    assert_eq!(discs.len(), 3, "three concentric discs");
    let mut widths: Vec<f32> = discs.iter().map(|(_, w)| *w).collect();
    widths.sort_by(|a, b| b.partial_cmp(a).unwrap());
    assert_eq!(widths, FLASH_DISC_HALF_WIDTHS.to_vec());

    let age = TICK_SECS;
    let frame = flash_frame(age).unwrap();
    for (disc, half_width) in discs {
        // A unit quad, flat in its own XY.
        let vs = h.local_vertices(disc);
        assert_eq!(vs.len(), 4);
        for v in &vs {
            assert!((v.x.abs() - 0.5).abs() < 1e-6 && (v.y.abs() - 0.5).abs() < 1e-6);
            assert!(v.z.abs() < 1e-6);
        }
        // Lying flat, facing up, centred on the feet at the model's height.
        let g = h.global(disc);
        let (scale, rotation, translation) = g.to_scale_rotation_translation();
        assert!(
            (rotation * Vec3::Z).dot(Vec3::Y) > 0.9999,
            "the disc lies flat"
        );
        let want = feet(at) + Vec3::Y * FLASH_ABOVE_FLOOR;
        assert!(translation.distance(want) < 1e-4, "{translation} vs {want}");
        // Its drawn diameter is the client half-width, doubled, on the bone
        // scale 16 ms into its 0.613 -> 3.243 bloom over 467 ms.
        let bloom = 0.613 + (3.243 - 0.613) * (16.0 / 467.0);
        assert!((frame.scale - bloom).abs() < 1e-4);
        let diameter = 2.0 * half_width * bloom;
        assert!(
            (scale.x - diameter).abs() < 1e-3,
            "{} vs {diameter}",
            scale.x
        );
        assert!((scale.y - diameter).abs() < 1e-3);

        // Additive, textured with the soft disc, lit by its emissive.
        let m = h.material(disc);
        assert_eq!(m.alpha_mode, AlphaMode::Add);
        assert!(!m.unlit, "unlit would discard the emissive");
        let texture = m.base_color_texture.clone().expect("textured");
        assert_eq!(m.emissive_texture.as_ref(), Some(&texture));
        let falloff = h.texture_alpha(&texture, &[0.0, 0.55, 0.98]);
        assert!(falloff[0] > 0.95, "{falloff:?}");
        assert!((falloff[1] - 0.55).abs() < 0.05, "{falloff:?}");
        assert!(falloff[2] < 0.05, "{falloff:?}");
        assert_rgb(m.base_color, TEAL, 6.0, "the flash opens teal");
        let want = srgb(frame.color);
        assert_rgb(
            m.base_color,
            [
                want[0].round() as u8,
                want[1].round() as u8,
                want[2].round() as u8,
            ],
            0.6,
            "the frame's colour",
        );
        assert!((m.base_color.alpha() - frame.disc_alpha).abs() < 1e-5);
        let e = m.emissive;
        let c = frame.color.to_linear();
        assert!((e.red - c.red * FLASH_GLOW).abs() < 1e-4);
        assert!((e.green - c.green * FLASH_GLOW).abs() < 1e-4);
        assert!((e.blue - c.blue * FLASH_GLOW).abs() < 1e-4);
    }
}

#[test]
fn the_flash_blooms_brightest_at_133_ms_turns_blue_and_is_gone_by_467_ms() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let warlock = h.warlock(at, false);
    h.land(warlock, AuraLandingKind::AmplifyFlash);
    h.tick(1); // spawned at age 0
    let outer = |h: &mut Harness| {
        let (disc, _) = h
            .discs()
            .into_iter()
            .find(|(_, w)| *w == FLASH_DISC_HALF_WIDTHS[0])
            .unwrap();
        disc
    };

    // 133 ms: brightest, 0.6 × 0.8 per disc.
    h.tick(8); // 8 × 16 ms = 128 ms
    let disc = outer(&mut h);
    let bright = h.material(disc).base_color.alpha();
    assert!((bright - 0.48).abs() < 0.02, "{bright}");

    // 464 ms: near the peak bloom (4.1 yd across), nearly blue, nearly gone.
    h.tick(20);
    let disc = outer(&mut h);
    let width = h.global(disc).to_scale_rotation_translation().0.x;
    assert!((width - AMPLIFY_FLASH_MAX_DIAMETER).abs() < 0.1, "{width}");
    let m = h.material(disc);
    assert_rgb(m.base_color, BLUE, 8.0, "the flash ends blue");
    assert!(m.base_color.alpha() < 0.02);

    // 480 ms: gone, discs and all.
    h.tick(1);
    assert!(h.flashes().is_empty());
    assert!(h.discs().is_empty());
}

#[test]
fn the_flash_follows_the_feet_and_plays_out_after_the_curse_spends_its_buff() {
    let mut h = Harness::new();
    let at = Vec3::new(1.0, COMBATANT_Y, 2.0);
    let warlock = h.warlock(at, false);
    h.app
        .world_mut()
        .get_mut::<ActiveAuras>(warlock)
        .unwrap()
        .auras
        .push(primed(AMPLIFY_CURSE_AURA));
    h.land(warlock, AuraLandingKind::AmplifyFlash);
    h.tick(2);
    // The curse spends Amplify Curse a frame after it lands.
    h.app
        .world_mut()
        .get_mut::<ActiveAuras>(warlock)
        .unwrap()
        .auras
        .clear();
    let moved = Vec3::new(3.0, COMBATANT_Y, -1.0);
    h.move_to(warlock, moved);
    h.tick(5);
    let flashes = h.flashes();
    assert_eq!(flashes.len(), 1, "the flash is a landing: it plays out");
    let t = h.global(flashes[0]).translation();
    assert!(t.distance(feet(moved) + Vec3::Y * FLASH_ABOVE_FLOOR) < 1e-4);
    assert!(h.halos().is_empty(), "Amplify Curse has no halo");

    // Even with Shadow Trance up, Amplify Curse's landing raises no halo.
    let mut h = Harness::new();
    let warlock = h.warlock(Vec3::new(0.0, COMBATANT_Y, 0.0), true);
    h.land(warlock, AuraLandingKind::AmplifyFlash);
    h.tick(5);
    assert_eq!(h.flashes().len(), 1);
    assert!(h.halos().is_empty(), "Amplify Curse has no halo");
    assert_eq!(h.particle_count(), 0);
}

#[test]
fn nothing_is_drawn_on_a_corpse() {
    let mut h = Harness::new();
    let warlock = h.warlock(Vec3::new(0.0, COMBATANT_Y, 0.0), true);
    h.kill(warlock);
    h.land(warlock, AuraLandingKind::AmplifyFlash);
    h.land(warlock, AuraLandingKind::ShadowTrance);
    h.tick(3);
    assert!(h.flashes().is_empty());
    assert!(h.halos().is_empty());
    assert_eq!(h.particle_count(), 0);
    // The markers were still consumed.
    let world = h.app.world_mut();
    assert_eq!(world.query::<&AuraLanding>().iter(world).count(), 0);
}

#[test]
fn a_warlock_dying_mid_proc_takes_the_flash_and_the_halo_with_it() {
    let mut h = Harness::new();
    let warlock = h.warlock(Vec3::new(0.0, COMBATANT_Y, 0.0), true);
    h.land(warlock, AuraLandingKind::ShadowTrance);
    h.tick(10);
    assert_eq!(h.flashes().len(), 1);
    assert_eq!(h.halos().len(), 1);
    assert!(h.particle_count() > 0, "the halo is emitting");
    h.kill(warlock);
    h.tick(1);
    assert!(h.flashes().is_empty());
    assert!(h.discs().is_empty());
    assert!(h.halos().is_empty());
    assert_eq!(h.particle_count(), 0, "no speck outlives its Warlock");
}

#[test]
fn shadow_trance_plays_the_flash_and_a_halo_of_dark_specks_at_the_head() {
    let mut h = Harness::new();
    let at = Vec3::new(-2.0, COMBATANT_Y, 5.0);
    let warlock = h.warlock(at, true);
    h.land(warlock, AuraLandingKind::ShadowTrance);
    h.tick(1);
    assert_eq!(h.flashes().len(), 1, "the same flash");
    assert_eq!(h.discs().len(), 3);
    let halos = h.halos();
    assert_eq!(halos.len(), 1);
    assert_eq!(halos[0].1, warlock);
    assert!(h.global(halos[0].0).translation().distance(head(at)) < 1e-4);

    // 12 a second: one second in, twelve specks.
    h.tick_secs(1.0);
    let specks = h.specks();
    assert!(
        (11..=13).contains(&specks.len()),
        "12/s for 1 s, saw {}",
        specks.len()
    );
    for (speck, position, _) in &specks {
        // Near-black, alpha-blended (it darkens the sand), on the hard blob.
        let m = h.material(*speck);
        assert_eq!(m.alpha_mode, AlphaMode::Blend);
        assert!(m.unlit);
        assert_rgb(m.base_color, SPECK_RGB, 0.5, "a speck");
        let texture = m.base_color_texture.clone().expect("textured");
        let blob = h.texture_alpha(&texture, &[0.0, 0.6, BLOB_EDGE + 0.06, 0.95]);
        assert!(blob[0] > 0.99 && blob[1] > 0.99, "a hard disc: {blob:?}");
        assert!(blob[2] < 0.01 && blob[3] < 0.01, "edged at 0.7: {blob:?}");
        // A unit quad drawn at the client's size: a 0.028 yd radius.
        let vs = h.local_vertices(*speck);
        assert_eq!(vs.len(), 4);
        let scale = h.global(*speck).to_scale_rotation_translation().0;
        assert!(
            (scale.x - 2.0 * SHADOW_TRANCE_SPECK_SIZE).abs() < 1e-5,
            "{}",
            scale.x
        );
        // About the head: within the shell's reach.
        assert!(position.distance(head(at)) < SPECK_SHELL_RADIUS + 0.2);
    }
}

#[test]
fn the_specks_drift_in_and_sink_and_follow_the_head() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let warlock = h.warlock(at, true);
    h.land(warlock, AuraLandingKind::ShadowTrance);
    h.tick(10);
    let before: Vec<(Entity, Vec3)> = h.specks().iter().map(|(e, p, _)| (*e, *p)).collect();
    assert!(!before.is_empty());
    h.tick_secs(1.0);
    let centre = head(at);
    for (speck, was) in &before {
        let now = h.global(*speck).translation();
        let radial = |p: Vec3| Vec2::new(p.x - centre.x, p.z - centre.z).length();
        assert!(
            radial(now) < radial(*was),
            "a speck drifts in toward the head"
        );
        assert!(now.y < was.y, "and sinks under the model's gravity");
    }
    // They fall under the model's 0.139 yd/s² gravity: the second difference
    // of a speck's height over two half-second steps.
    let speck = before[0].0;
    let y0 = h.global(speck).translation().y;
    h.tick_secs(0.5);
    let y1 = h.global(speck).translation().y;
    h.tick_secs(0.5);
    let y2 = h.global(speck).translation().y;
    let accel = (y2 - 2.0 * y1 + y0) / 0.25;
    assert!(
        (accel + 0.139).abs() < 0.01,
        "a speck accelerates {accel} yd/s²"
    );
    // The halo rides the head: a step moves every speck with it.
    let speck = before[0].0;
    let was = h.global(speck).translation();
    let step = Vec3::new(2.0, 0.0, -1.0);
    h.move_to(warlock, at + step);
    h.tick(1);
    let now = h.global(speck).translation();
    // One frame of drift is under a centimetre.
    assert!((now - was - step).length() < 0.02, "{}", (now - was - step));
}

#[test]
fn the_specks_face_a_rotated_camera() {
    let mut h = Harness::new();
    let warlock = h.warlock(Vec3::new(0.0, COMBATANT_Y, 0.0), true);
    h.land(warlock, AuraLandingKind::ShadowTrance);
    h.tick_secs(0.5);
    let to_camera = h.global(h.camera).rotation() * Vec3::Z;
    let specks = h.specks();
    assert!(!specks.is_empty());
    for (speck, _, _) in specks {
        let normal = h.global(speck).rotation() * Vec3::Z;
        assert!(
            normal.dot(to_camera) > 0.999,
            "a speck is not facing the camera ({})",
            normal.dot(to_camera)
        );
    }
}

/// A Warlock waiting on Shadow Trance with two other auras that stay up when
/// the bolt spends it: a primed Amplify Curse (another next-cast aura) and a
/// Fortitude-style stat buff.
fn waiting_with_company(h: &mut Harness) -> Entity {
    let warlock = h.warlock(Vec3::new(0.0, COMBATANT_Y, 0.0), true);
    let mut auras = h.app.world_mut().get_mut::<ActiveAuras>(warlock).unwrap();
    auras.auras.push(primed(AMPLIFY_CURSE_AURA));
    auras.auras.push(Aura {
        effect_type: AuraType::MaxHealthIncrease,
        duration: 1800.0,
        magnitude: 50.0,
        ability_name: "Power Word: Fortitude".into(),
        ..Default::default()
    });
    warlock
}

#[test]
fn the_halo_goes_the_moment_shadow_trance_is_spent() {
    // Control: still waiting, the halo stays.
    let mut kept = Harness::new();
    let w = waiting_with_company(&mut kept);
    kept.land(w, AuraLandingKind::ShadowTrance);
    kept.tick_secs(1.5);
    kept.tick(1);
    assert_eq!(kept.halos().len(), 1);
    assert!(kept.particle_count() > 0);

    // Spent: only Shadow Trance comes off. The other next-cast aura and the
    // stat buff stay up, and neither holds the halo.
    let mut h = Harness::new();
    let warlock = waiting_with_company(&mut h);
    h.land(warlock, AuraLandingKind::ShadowTrance);
    h.tick_secs(1.5);
    assert!(h.particle_count() > 0);
    h.spend_trance(warlock);
    assert_eq!(
        h.app
            .world()
            .get::<ActiveAuras>(warlock)
            .unwrap()
            .auras
            .len(),
        2,
        "Amplify Curse and the stat buff are still up"
    );
    h.tick(1);
    if SHADOW_TRANCE_HALO_ENDS_WITH_PROC {
        assert!(h.halos().is_empty());
        assert_eq!(h.particle_count(), 0, "no speck outlives the proc");
    } else {
        // The client's one-shot: the spend does not touch it.
        assert_eq!(h.halos().len(), 1);
        assert!(h.particle_count() > 0);
    }
}

/// Two Nightfall procs landing in one frame (two Corruption ticks) raise one
/// halo, emitting at one halo's rate.
#[test]
fn two_procs_in_one_frame_raise_one_halo() {
    let mut h = Harness::new();
    let warlock = h.warlock(Vec3::new(0.0, COMBATANT_Y, 0.0), true);
    h.land(warlock, AuraLandingKind::ShadowTrance);
    h.land(warlock, AuraLandingKind::ShadowTrance);
    h.tick(1);
    assert_eq!(h.halos().len(), 1, "one halo per Warlock");
    h.tick_secs(1.0);
    assert_eq!(h.halos().len(), 1);
    let specks = h.specks().len();
    assert!((11..=13).contains(&specks), "12 a second, saw {specks}");
}

#[test]
fn the_halo_emits_for_its_window_then_retires_after_its_last_speck() {
    let mut h = Harness::new();
    let warlock = h.warlock(Vec3::new(0.0, COMBATANT_Y, 0.0), true);
    h.land(warlock, AuraLandingKind::ShadowTrance);
    h.tick_secs(6.0);
    // 12 a second for 4.5 s (the last one may fall to the float sum).
    let emitted = h.halo_emitted();
    assert!((53..=54).contains(&emitted), "{emitted}");
    assert_eq!(h.halos().len(), 1, "its last specks are still alive");
    // Each speck lives its 4 s: the oldest still alive at 6 s was born
    // about 2 s in.
    let oldest = h
        .specks()
        .iter()
        .map(|(_, _, age)| *age)
        .fold(0.0, f32::max);
    assert!(
        (3.8..4.0).contains(&oldest),
        "the oldest speck is {oldest} s old"
    );
    h.tick_secs(2.6); // past 8.5 s
    assert!(h.halos().is_empty());
    assert_eq!(h.particle_count(), 0);
}

#[test]
fn a_proc_that_refreshes_shadow_trance_restarts_the_one_halo() {
    let mut h = Harness::new();
    let warlock = h.warlock(Vec3::new(0.0, COMBATANT_Y, 0.0), true);
    h.land(warlock, AuraLandingKind::ShadowTrance);
    h.tick_secs(4.0);
    h.land(warlock, AuraLandingKind::ShadowTrance);
    h.tick_secs(4.0);
    assert_eq!(h.halos().len(), 1, "one halo per Warlock");
    // 4 s of the first proc, then 4 more of the second: 96, never two halos'
    // worth.
    let emitted = h.halo_emitted();
    assert!((95..=97).contains(&emitted), "{emitted}");
    assert_eq!(h.flashes().len(), 0, "both flashes have played out");
}

#[test]
fn another_renderers_landing_is_left_alone() {
    let mut h = Harness::new();
    let warlock = h.warlock(Vec3::new(0.0, COMBATANT_Y, 0.0), false);
    h.land(warlock, AuraLandingKind::ShadowWordPainHit);
    h.tick(2);
    assert!(h.flashes().is_empty());
    let world = h.app.world_mut();
    let left: Vec<AuraLandingKind> = world
        .query::<&AuraLanding>()
        .iter(world)
        .map(|l| l.kind)
        .collect();
    assert_eq!(left, vec![AuraLandingKind::ShadowWordPainHit]);
}

// ── through the default match camera ─────────────────────────────────────────

/// The match camera's first-frame placement (`camera.rs`): 60 yd out, 38.7°
/// off vertical, looking at the fight; the client's default 45° projection at
/// 1920 × 1080.
struct MatchCamera {
    clip_from_world: Mat4,
    eye: Vec3,
}

const SCREEN: Vec2 = Vec2::new(1920.0, 1080.0);

impl MatchCamera {
    fn looking_at(target: Vec3) -> Self {
        let pitch = 38.7f32.to_radians();
        let eye = target + Vec3::new(0.0, 60.0 * pitch.cos(), 60.0 * pitch.sin());
        let view = Transform::from_translation(eye).looking_at(target, Vec3::Y);
        let projection = PerspectiveProjection {
            aspect_ratio: SCREEN.x / SCREEN.y,
            ..default()
        };
        assert_eq!(
            projection.fov,
            PI / 4.0,
            "the client's camera is Bevy's default"
        );
        let clip_from_view = projection.get_clip_from_view();
        Self {
            clip_from_world: clip_from_view * view.compute_matrix().inverse(),
            eye,
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

    /// Whether the body capsule of a unit at `at` (radius 0.5, 2.5 tall)
    /// stands between the camera and `p`.
    fn hidden_by_body(&self, p: Vec3, at: Vec3) -> bool {
        let (bottom, top) = (at - Vec3::Y * 0.75, at + Vec3::Y * 0.75);
        (1..200).any(|i| {
            let q = self.eye.lerp(p, i as f32 / 200.0);
            let axis = q.clamp(Vec3::new(q.x, bottom.y, q.z), Vec3::new(q.x, top.y, q.z));
            let on_axis = Vec3::new(at.x, axis.y, at.z);
            q.distance(on_axis) < 0.5 && q.distance(p) > 1e-3
        })
    }
}

/// At the match camera the flash's peak spans about 90 px, four times the
/// body it blooms under, so it reads round the Warlock rather than hiding
/// beneath it.
#[test]
fn the_flash_reads_round_the_body_at_the_match_camera() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let warlock = h.warlock(at, false);
    h.land(warlock, AuraLandingKind::AmplifyFlash);
    h.tick(29); // 464 ms: the peak bloom
    let (disc, _) = h
        .discs()
        .into_iter()
        .find(|(_, w)| *w == FLASH_DISC_HALF_WIDTHS[0])
        .unwrap();
    let g = h.global(disc);
    let cam = MatchCamera::looking_at(at);
    let rim: Vec<Vec2> = h
        .local_vertices(disc)
        .iter()
        .map(|v| cam.pixel(g.transform_point(*v * Vec3::new(1.0, 1.0, 0.0))))
        .collect();
    let width = rim.iter().map(|p| p.x).fold(f32::MIN, f32::max)
        - rim.iter().map(|p| p.x).fold(f32::MAX, f32::min);
    let body = (cam.pixel(at + Vec3::X * 0.5) - cam.pixel(at - Vec3::X * 0.5)).length();
    assert!(width > 80.0, "the flash spans {width} px at its peak");
    assert!(width > 3.5 * body, "flash {width} px vs body {body} px");
    // Its near rim, toward the camera, lies outside the body's footprint.
    let near = g.translation() + Vec3::Z * (AMPLIFY_FLASH_MAX_DIAMETER / 2.0 - 0.1);
    assert!(!cam.hidden_by_body(near, at));
}

/// The specks are kept at client size (ruling, 2026-10-17): about a pixel
/// across at the match camera.
#[test]
fn the_specks_are_client_size_at_the_match_camera() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let warlock = h.warlock(at, true);
    h.land(warlock, AuraLandingKind::ShadowTrance);
    h.tick(30);
    let cam = MatchCamera::looking_at(at);
    let specks = h.specks();
    assert!(specks.len() >= 5);
    for (speck, position, _) in &specks {
        let size = h.global(*speck).to_scale_rotation_translation().0.x;
        let px = (cam.pixel(*position + Vec3::X * size * 0.5)
            - cam.pixel(*position - Vec3::X * size * 0.5))
        .length();
        assert!((0.8..2.0).contains(&px), "a speck draws {px} px wide");
    }
}

/// The halo's shell rings the crown outside the body, so from the match
/// camera's height nearly every speck is born in sight rather than inside the
/// head.
#[test]
fn the_specks_are_born_in_sight_of_the_match_camera() {
    let at = Vec3::new(0.0, COMBATANT_Y, 0.0);
    let cam = MatchCamera::looking_at(at);
    let n = 1000;
    let in_sight = (0..n)
        .filter(|seed| !cam.hidden_by_body(head(at) + speck_launch(*seed).0, at))
        .count();
    assert!(
        in_sight * 100 >= n as usize * 95,
        "{in_sight} of {n} in sight"
    );
}
