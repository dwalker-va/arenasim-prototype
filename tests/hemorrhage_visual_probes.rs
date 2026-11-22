//! Probes for Hemorrhage's visual (AS-367): Sinister Strike's stroke, and
//! Rupture's bleed spray (AS-365) at the victim's chest on every hit that
//! lands, drawn in front of the victim's body. A dodged or parried
//! Hemorrhage strokes and sprays nothing; the stacking wound raises no band.
//!
//! Most of the card is REUSE, so the probes pin that Hemorrhage reaches the
//! same stroke and the same spray as their owners through the real graphical
//! chain (the router, the avoided-strike path, the swing, the spray, the
//! shared particle animator, the band detector), asserting what is DRAWN —
//! world positions, materials, meshes, sprites — never stored fields alone.
//! The spray's own internals (its count, cone, palette, sprite) are AS-365's
//! and pinned in `rupture_visual_probes.rs`; what is pinned here is the
//! repeat case Hemorrhage brings (a spray per hit, overlapping, refreshing),
//! the hold in front of the body, and the spray's lifecycle beside other
//! auras. The real schedule — the drain's marker, the sim's `MeleeAvoided` —
//! is `hemorrhage_match_flow.rs`.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU.

use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_book::AbilityBook;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::combat_core::{Avoidance, MeleeAvoided};
use arenasim::states::play_match::components::{
    ActiveAuras, Aura, AuraApplyOwner, AuraApplyRoute, AuraBand, AuraPending, AuraType,
    ClientParticle, Combatant, InstantAbilityFired, Pet, PetType, PlayMatchEntity, SwingStyle,
    VisualBody, WeaponHand, WeaponKind, WeaponSocket, HEMORRHAGE_AURA,
};
use arenasim::states::play_match::{
    animate_body_lean, animate_client_particles, animate_weapon_swings, billboard_bleed_droplets,
    cleanup_play_match, consume_avoided_strike_signals, consume_instant_ability_signals,
    detect_aura_applications, fire_bleed_sprays, note_avoided_strikes, swing_style_for_ability,
    update_bleed_sprays, BleedDroplet, BleedSpray, BleedSprayPending, SIPHON_BODY_CLEARANCE,
    STROKES_WHEN_AVOIDED,
};
use arenasim::CharacterClass;

/// 10 ms frames: fine enough to sample the stroke.
const TICK: Duration = Duration::from_millis(10);

/// The Rogue at the origin, its victim 2 yd ahead on +Z.
const ROGUE_POS: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const VICTIM_POS: Vec3 = Vec3::new(0.0, 1.0, 2.0);
/// `IMPACT_CHEST_Y` above a standing unit's origin.
const CHEST_ABOVE: f32 = 0.55;
/// A pet's chest: its body centre (`IMPACT_PET_BODY_Y`, 0.45 below its
/// origin) plus the chest height at the pet's stature (0.55 × 0.55).
const PET_CHEST_ABOVE: f32 = -0.45 + 0.55 * 0.55;

/// The shipped bodies the spray is drawn in front of: a combatant's capsule
/// (radius 0.5, its segment 0.75 either side of its origin), a pet's sphere
/// (radius 0.65 round a centre 0.45 below its origin).
fn body_capsule(at: Vec3, pet: bool) -> (Vec3, Vec3, f32) {
    if pet {
        let c = at - Vec3::Y * 0.45;
        (c, c, 0.65)
    } else {
        (at - Vec3::Y * 0.75, at + Vec3::Y * 0.75, 0.5)
    }
}

/// The match camera: 20 yd off at a 55° pitch, looking at `target`.
fn match_camera(target: Vec3, yaw: f32) -> Transform {
    let (zoom, pitch) = (20.0_f32, 55.0_f32.to_radians());
    let at = target
        + Vec3::new(
            zoom * pitch.sin() * yaw.sin(),
            zoom * pitch.cos(),
            zoom * pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

fn dagger_mount(hand: WeaponHand) -> Transform {
    let side = if hand == WeaponHand::Off { -1.0 } else { 1.0 };
    Transform::from_xyz(0.7 * side, -0.05, 0.3)
        .with_rotation(
            Quat::from_rotation_x(0.9) * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        )
        .with_scale(Vec3::splat(0.8))
}

/// The resolution phase's retirement of avoidance markers.
fn retire_avoidances(mut commands: Commands, avoided: Query<Entity, With<MeleeAvoided>>) {
    for e in avoided.iter() {
        commands.entity(e).despawn();
    }
}

fn defs() -> AbilityDefinitions {
    AbilityDefinitions::default()
}

/// The aura `ability`'s own config lands on `victim` from `caster`, as
/// `apply_pending_auras` builds it.
fn aura_of(ability: AbilityType, victim: Entity, caster: Entity) -> Aura {
    AuraPending::from_ability(victim, caster, defs().get(&ability).unwrap())
        .unwrap_or_else(|| panic!("{ability:?} applies an aura"))
        .aura
}

struct Harness {
    app: App,
    rogue: Entity,
    victim: Entity,
    main: Entity,
}

impl Harness {
    /// A Subtlety Rogue holding Hemorrhage and Rupture, and its victim — a
    /// Warrior, or a pet. No camera until [`Harness::camera`].
    fn new(pet_victim: bool) -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            TransformPlugin,
        ));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
        app.insert_resource(defs());
        // The client's order (`states/mod.rs`): the avoidance noted before the
        // resolution phase retires it, both routers after it, the swing, the
        // band detector off `ActiveAuras`, then the spray — emitted before the
        // shared animator flies it, held and turned after.
        app.add_systems(
            Update,
            (
                note_avoided_strikes,
                retire_avoidances,
                consume_instant_ability_signals,
                consume_avoided_strike_signals,
                animate_weapon_swings,
                animate_body_lean,
                detect_aura_applications,
                fire_bleed_sprays,
                update_bleed_sprays,
                animate_client_particles,
                billboard_bleed_droplets,
            )
                .chain(),
        );
        let book = AbilityBook::for_class(&defs(), CharacterClass::Rogue)
            .granting(&defs(), AbilityType::Hemorrhage)
            .granting(&defs(), AbilityType::Rupture);
        let rogue = app
            .world_mut()
            .spawn((
                Transform::from_translation(ROGUE_POS),
                Combatant::new(1, 0, CharacterClass::Rogue).with_book(book),
            ))
            .id();
        let body = app
            .world_mut()
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        let socket = |app: &mut App, hand: WeaponHand| {
            let rest = dagger_mount(hand);
            app.world_mut()
                .spawn((
                    WeaponSocket {
                        kind: WeaponKind::Dagger,
                        hand,
                        owner: rogue,
                        rest,
                        release_t: None,
                        aim: VICTIM_POS,
                        yaw_local: 0.0,
                        prev_owner_yaw: 0.0,
                        windup_s: 0.0,
                        swing_style: SwingStyle::Auto,
                        stroke_interval: 0.0,
                        last_s: 0.0,
                    },
                    rest,
                    Visibility::default(),
                ))
                .id()
        };
        let main = socket(&mut app, WeaponHand::Main);
        let off = socket(&mut app, WeaponHand::Off);
        app.world_mut().entity_mut(body).add_children(&[main, off]);
        app.world_mut().entity_mut(rogue).add_child(body);
        let victim = if pet_victim {
            let owner = app
                .world_mut()
                .spawn((
                    Transform::from_xyz(30.0, 1.0, 30.0),
                    Combatant::new(2, 0, CharacterClass::Hunter),
                ))
                .id();
            app.world_mut()
                .spawn((
                    Transform::from_translation(VICTIM_POS),
                    Combatant::new(2, 1, CharacterClass::Hunter),
                    Pet {
                        owner,
                        pet_type: PetType::Boar,
                    },
                    ActiveAuras { auras: Vec::new() },
                ))
                .id()
        } else {
            app.world_mut()
                .spawn((
                    Transform::from_translation(VICTIM_POS),
                    Combatant::new(2, 0, CharacterClass::Warrior),
                    ActiveAuras { auras: Vec::new() },
                ))
                .id()
        };
        Harness {
            app,
            rogue,
            victim,
            main,
        }
    }

    fn camera(&mut self, cam: Transform) {
        self.app.world_mut().spawn((Camera3d::default(), cam));
    }

    /// The drain's marker for a landed `ability` on the victim.
    fn land(&mut self, ability: AbilityType) {
        let (rogue, victim) = (self.rogue, self.victim);
        self.app.world_mut().spawn((
            InstantAbilityFired {
                caster: rogue,
                target: Some(victim),
                ability,
                is_crit: false,
            },
            PlayMatchEntity,
        ));
    }

    /// The sim's marker for a Hemorrhage the victim dodged or parried.
    fn avoid(&mut self, avoidance: Avoidance) {
        let (rogue, victim) = (self.rogue, self.victim);
        self.app.world_mut().spawn(MeleeAvoided {
            attacker: rogue,
            defender: victim,
            attack: "Hemorrhage".to_string(),
            avoidance,
        });
    }

    fn auras(&mut self) -> Mut<'_, ActiveAuras> {
        let victim = self.victim;
        self.app.world_mut().get_mut::<ActiveAuras>(victim).unwrap()
    }

    /// The Rogue's wound lands — or, worn already, refreshes and stacks, as
    /// `apply_pending_auras` refreshes a stacking aura.
    fn wound(&mut self) {
        let (rogue, victim) = (self.rogue, self.victim);
        let fresh = aura_of(AbilityType::Hemorrhage, victim, rogue);
        let mut auras = self.auras();
        match auras
            .auras
            .iter_mut()
            .find(|a| a.caster == Some(rogue) && a.ability_name == HEMORRHAGE_AURA)
        {
            Some(worn) => {
                worn.duration = fresh.duration;
                let stacks = worn.stacks.as_mut().expect("the wound stacks");
                stacks.count = (stacks.count + 1).min(stacks.max);
            }
            None => auras.auras.push(fresh),
        }
    }

    /// Rupture from the Rogue, Mortal Strike's wound from a Warrior and a
    /// Crippling Poison: the auras a Hemorrhage victim wears beside it.
    fn other_auras(&mut self) {
        let (rogue, victim) = (self.rogue, self.victim);
        let warrior = self
            .app
            .world_mut()
            .spawn(Combatant::new(1, 1, CharacterClass::Warrior))
            .id();
        let others = [
            aura_of(AbilityType::Rupture, victim, rogue),
            aura_of(AbilityType::MortalStrike, victim, warrior),
            aura_of(AbilityType::CripplingPoison, victim, rogue),
        ];
        self.auras().auras.extend(others);
    }

    fn tick(&mut self) {
        self.app.update();
    }

    fn ticks(&mut self, n: usize) {
        for _ in 0..n {
            self.tick();
        }
    }

    /// Ticks until a spray has fired, and panics if none fires within a
    /// second.
    fn run_to_spray(&mut self) {
        for _ in 0..100 {
            if self.count::<BleedSpray>() > 0 {
                return;
            }
            self.tick();
        }
        panic!("no spray fired");
    }

    fn count<C: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query_filtered::<(), With<C>>();
        q.iter(self.app.world()).count()
    }

    /// Every bleed entity: held sprays, sprays and droplets.
    fn bleed(&mut self) -> usize {
        self.count::<BleedSprayPending>()
            + self.count::<BleedSpray>()
            + self.count::<BleedDroplet>()
    }

    fn droplets(&mut self) -> Vec<(Entity, Vec3, Vec3, f32, f32)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(Entity, &BleedDroplet, &Transform, &ClientParticle)>();
        q.iter(self.app.world())
            .map(|(e, d, tf, p)| (e, d.truly, tf.translation, tf.scale.x, p.age))
            .collect()
    }

    fn material(&self, e: Entity) -> StandardMaterial {
        let handle = &self
            .app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(e)
            .unwrap()
            .0;
        self.app
            .world()
            .resource::<Assets<StandardMaterial>>()
            .get(handle)
            .unwrap()
            .clone()
    }

    fn grip(&self) -> Vec3 {
        self.app
            .world()
            .get::<GlobalTransform>(self.main)
            .unwrap()
            .translation()
    }

    fn socket(&self) -> &WeaponSocket {
        self.app.world().get::<WeaponSocket>(self.main).unwrap()
    }

    fn kill_victim(&mut self) {
        let victim = self.victim;
        self.app
            .world_mut()
            .get_mut::<Combatant>(victim)
            .unwrap()
            .current_health = 0.0;
    }

    /// Plays `secs`, counting distinct droplets and the most sprays live at
    /// once, and returns `(distinct droplets, most live sprays)`.
    fn play(&mut self, secs: f32) -> (usize, usize) {
        let mut seen = std::collections::HashSet::new();
        let mut most = 0;
        for _ in 0..(secs / TICK.as_secs_f32()).round() as usize {
            self.tick();
            let mut q = self
                .app
                .world_mut()
                .query_filtered::<Entity, With<BleedDroplet>>();
            seen.extend(q.iter(self.app.world()));
            most = most.max(self.count::<BleedSpray>());
        }
        (seen.len(), most)
    }
}

/// The spray's rate track, 274.95 droplets: 274 whole ones.
const DROPLETS: usize = 274;

// ── The routes ───────────────────────────────────────────────────────────────

/// Hemorrhage takes Sinister Strike's place in the rotation, so it swings
/// Sinister Strike's stroke (the ruling), not the client's Special1H.
#[test]
fn hemorrhage_swings_sinister_strikes_stroke() {
    assert_eq!(
        swing_style_for_ability(AbilityType::Hemorrhage),
        Some(SwingStyle::SinisterStrike)
    );
    assert_eq!(
        swing_style_for_ability(AbilityType::SinisterStrike),
        Some(SwingStyle::SinisterStrike)
    );
}

/// The wound's application is owned by its strike's spray, by NAME; its type
/// is the damage pipeline's, so another damage-taken increase keeps the
/// band, and the name alone owns no other type. The name is the RON's.
#[test]
fn the_wound_is_owned_by_its_spray_and_its_type_siblings_keep_the_band() {
    let def = defs();
    let def = def.get_unchecked(&AbilityType::Hemorrhage);
    assert_eq!(def.name, HEMORRHAGE_AURA, "the RON name");
    let t = def.applies_aura.as_ref().unwrap().aura_type;
    assert_eq!(t, AuraType::DamageTakenIncrease);
    assert_eq!(
        AuraApplyRoute::for_instance(t, &def.name),
        AuraApplyRoute::Owned(AuraApplyOwner::BleedSpray)
    );
    assert_eq!(
        AuraApplyRoute::for_instance(t, "Shadow Vulnerability"),
        AuraApplyRoute::Band,
        "another damage-taken increase keeps the band"
    );
    for other in AuraType::ALL
        .into_iter()
        .filter(|o| *o != AuraType::DamageTakenIncrease)
    {
        assert_eq!(
            AuraApplyRoute::for_instance(other, HEMORRHAGE_AURA),
            AuraApplyRoute::for_aura(other),
            "{other:?}"
        );
    }
}

/// The strikes that still swing when avoided: Crusader Strike's and
/// Hemorrhage's, by their cards; no other.
#[test]
fn hemorrhage_strokes_when_avoided_beside_crusader_strike_only() {
    let mut avoided: Vec<String> = STROKES_WHEN_AVOIDED
        .iter()
        .map(|a| format!("{a:?}"))
        .collect();
    avoided.sort();
    assert_eq!(avoided, ["CrusaderStrike", "Hemorrhage"]);
}

// ── The stroke ───────────────────────────────────────────────────────────────

/// `(stroke seconds, main grip in world space)` each frame of the stroke the
/// landed `ability` swings, from its marker.
fn stroke_of(ability: AbilityType) -> Vec<(Option<f32>, SwingStyle, Vec3)> {
    let mut h = Harness::new(false);
    h.land(ability);
    let mut out = Vec::new();
    for _ in 0..80 {
        h.tick();
        out.push((h.socket().release_t, h.socket().swing_style, h.grip()));
    }
    out
}

/// A landed Hemorrhage swings Sinister Strike's stroke, frame for frame:
/// the same grip in the world on every frame, through the real router.
#[test]
fn a_landed_hemorrhage_swings_sinister_strikes_stroke_frame_for_frame() {
    let hemo = stroke_of(AbilityType::Hemorrhage);
    let sinister = stroke_of(AbilityType::SinisterStrike);
    assert_eq!(hemo[0].1, SwingStyle::SinisterStrike);
    assert!(
        hemo[0].0.is_some(),
        "the stroke starts on the marker's frame"
    );
    let travel = hemo
        .iter()
        .map(|(_, _, g)| g.distance(hemo[0].2))
        .fold(0.0_f32, f32::max);
    assert!(travel > 0.3, "the grip moves: {travel}");
    for (i, (a, b)) in hemo.iter().zip(&sinister).enumerate() {
        assert_eq!((a.0, a.1), (b.0, b.1), "frame {i}");
        assert!(a.2.distance(b.2) < 1e-6, "frame {i}: {} vs {}", a.2, b.2);
    }
}

// ── The spray ────────────────────────────────────────────────────────────────

/// A landed Hemorrhage holds the spray for Sinister Strike's contact, then
/// fires it once from its victim's chest.
#[test]
fn a_landed_hemorrhage_sprays_rupture_spray_from_the_chest_at_contact() {
    let contact = SwingStyle::SinisterStrike.impact_at();
    let mut h = Harness::new(false);
    h.wound();
    h.land(AbilityType::Hemorrhage);
    let mut early = Vec::new();
    let mut fired_at = None;
    let mut t = 0.0;
    for _ in 0..40 {
        h.tick();
        t += TICK.as_secs_f32();
        if h.count::<BleedSpray>() > 0 && fired_at.is_none() {
            fired_at = Some(t);
            early = h.droplets();
        }
    }
    let fired_at = fired_at.expect("the spray fired");
    // Its countdown starts the frame it is held; the app's first frame has
    // no time step, so it fires within two frames past the contact.
    assert!(
        fired_at >= contact && fired_at <= contact + 2.0 * TICK.as_secs_f32() + 1e-4,
        "fired at {fired_at}, contact {contact}"
    );
    assert!(!early.is_empty());
    let chest = VICTIM_POS + Vec3::Y * CHEST_ABOVE;
    for (_, truly, ..) in &early {
        assert!(truly.distance(chest) < 0.1, "{truly} is not at the chest");
    }

    // One spray, and it retires.
    let (_, most) = h.play(1.0);
    assert_eq!(most, 1);
    assert_eq!(h.bleed(), 0, "the spray retired");
}

/// The same spray as Rupture's, counted whole: a Hemorrhage and a Rupture
/// each fire 274 droplets.
#[test]
fn a_hemorrhage_spray_is_as_many_droplets_as_a_rupture_spray() {
    let count = |ability: AbilityType| {
        let mut h = Harness::new(false);
        let (rogue, victim) = (h.rogue, h.victim);
        h.auras().auras.push(aura_of(ability, victim, rogue));
        h.land(ability);
        h.play(1.5).0
    };
    assert_eq!(count(AbilityType::Hemorrhage), DROPLETS);
    assert_eq!(count(AbilityType::Rupture), DROPLETS);
}

/// Read back every droplet's drawn material and mesh `age` seconds after
/// contact, keyed by its age.
fn drawn_droplets(ability: AbilityType) -> Vec<(f32, StandardMaterial, Vec<Vec3>, f32)> {
    let mut h = Harness::new(false);
    let (rogue, victim) = (h.rogue, h.victim);
    let wound = match ability {
        AbilityType::Hemorrhage => aura_of(AbilityType::Hemorrhage, victim, rogue),
        _ => aura_of(AbilityType::Rupture, victim, rogue),
    };
    h.auras().auras.push(wound);
    h.land(ability);
    // Until the spray is a tenth of a second old.
    for _ in 0..200 {
        h.tick();
        if h.droplets().iter().any(|d| d.4 >= 0.1) {
            break;
        }
    }
    let mut out = Vec::new();
    for (e, _, _, scale, age) in h.droplets() {
        let m = h.material(e);
        let mesh = h
            .app
            .world()
            .resource::<Assets<Mesh>>()
            .get(&h.app.world().get::<Mesh3d>(e).unwrap().0)
            .unwrap();
        let v = match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            VertexAttributeValues::Float32x3(v) => v.iter().map(|p| Vec3::from(*p)).collect(),
            other => panic!("unexpected positions {other:?}"),
        };
        out.push((age, m, v, scale));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// Every Hemorrhage droplet is drawn exactly as a Rupture droplet of the same
/// age: the same colour, alpha, blend and lighting, the same quad, the same
/// size (no camera here, so nothing is held).
#[test]
fn a_hemorrhage_droplet_is_drawn_as_a_rupture_droplet() {
    let hemo = drawn_droplets(AbilityType::Hemorrhage);
    let rupture = drawn_droplets(AbilityType::Rupture);
    assert!(hemo.len() > 30, "{}", hemo.len());
    assert_eq!(hemo.len(), rupture.len());
    for (a, b) in hemo.iter().zip(&rupture) {
        assert!((a.0 - b.0).abs() < 1e-6);
        assert_eq!(a.1.base_color, b.1.base_color, "age {}", a.0);
        assert_eq!(a.1.alpha_mode, b.1.alpha_mode);
        assert_eq!(a.1.unlit, b.1.unlit);
        assert_eq!(
            a.1.base_color_texture.is_some(),
            b.1.base_color_texture.is_some()
        );
        assert_eq!(a.2, b.2, "the quad");
        assert!((a.3 - b.3).abs() < 1e-6, "the size");
    }
    // Rupture's deep-red alpha-blended disc, not a glow.
    let (_, m, v, _) = &hemo[0];
    assert_eq!(m.alpha_mode, AlphaMode::Blend);
    let c = m.base_color.to_srgba();
    assert!(c.red > 0.4 && c.green < 0.04 && c.blue < 0.04, "{c:?}");
    let extent = v.iter().fold(Vec3::ZERO, |a, p| a.max(p.abs()));
    assert!((extent.x - 0.5).abs() < 1e-6 && (extent.y - 0.5).abs() < 1e-6);
}

/// A dodged or parried Hemorrhage swings Sinister Strike's stroke exactly
/// once, and sprays nothing — even into a wound the victim already wears,
/// where a spray gated on the wound alone would fire.
#[test]
fn an_avoided_hemorrhage_strokes_once_and_sprays_nothing() {
    for avoidance in [Avoidance::Dodge, Avoidance::Parry] {
        let mut h = Harness::new(false);
        h.wound();
        h.avoid(avoidance);
        let mut starts = 0;
        let mut last = None;
        let mut bleed = 0;
        for _ in 0..100 {
            h.tick();
            let now = h.socket().release_t;
            if now.is_some_and(|t| last.is_none_or(|l| t < l)) {
                starts += 1;
                assert_eq!(h.socket().swing_style, SwingStyle::SinisterStrike);
            }
            last = now;
            bleed += h.bleed();
        }
        assert_eq!(starts, 1, "{avoidance:?}: one stroke");
        assert_eq!(bleed, 0, "{avoidance:?}: no blood");
    }
}

/// Every hit sprays its own spray, a refresh of the stacks included, and the
/// sprays overlap without sharing or resetting each other: three hits 0.2 s
/// apart are three whole sprays, two live at once. The wound's landings and
/// refreshes raise no band.
#[test]
fn every_hit_sprays_its_own_whole_spray_and_a_refresh_adds_none() {
    let mut h = Harness::new(false);
    let mut seen = std::collections::HashSet::new();
    let mut most = 0;
    let mut bands = 0;
    for hit in 0..3 {
        h.wound();
        h.land(AbilityType::Hemorrhage);
        for _ in 0..30 {
            h.tick();
            let mut q = h
                .app
                .world_mut()
                .query_filtered::<Entity, With<BleedDroplet>>();
            seen.extend(q.iter(h.app.world()));
            most = most.max(h.count::<BleedSpray>());
            bands += h.count::<AuraBand>();
        }
        assert_eq!(
            h.auras().auras[0].stacks.unwrap().count,
            hit + 1,
            "the wound stacks with each hit"
        );
    }
    for _ in 0..100 {
        h.tick();
        let mut q = h
            .app
            .world_mut()
            .query_filtered::<Entity, With<BleedDroplet>>();
        seen.extend(q.iter(h.app.world()));
    }
    let mut q = h
        .app
        .world_mut()
        .query_filtered::<Entity, With<BleedDroplet>>();
    seen.extend(q.iter(h.app.world()));
    assert_eq!(seen.len(), 3 * DROPLETS, "three whole sprays");
    assert_eq!(most, 2, "the sprays overlap");
    assert_eq!(bands, 0, "the wound raises no band");
    assert_eq!(h.bleed(), 0, "every spray retired");

    // The control: the band detector is live, and another damage-taken
    // increase raises its band.
    let mut h = Harness::new(false);
    let mut vuln = aura_of(AbilityType::Hemorrhage, h.victim, h.rogue);
    vuln.ability_name = "Shadow Vulnerability".to_string();
    h.auras().auras.push(vuln);
    h.ticks(2);
    assert_eq!(h.count::<AuraBand>(), 1);
}

// ── Drawn in front of the body ───────────────────────────────────────────────

/// Every droplet, every frame, at the match camera from `yaw`: drawn on the
/// view ray through where it truly is; never deeper than the victim's body's
/// nearest point; exactly where it is when already nearer; and at the
/// apparent size it has where it truly is.
fn assert_held_in_front(pet: bool, yaw: f32) -> (usize, usize) {
    let mut h = Harness::new(pet);
    let cam = match_camera(VICTIM_POS, yaw);
    h.camera(cam);
    h.wound();
    h.land(AbilityType::Hemorrhage);
    let (eye, fwd) = (cam.translation, cam.forward().as_vec3());
    let (a, b, r) = body_capsule(VICTIM_POS, pet);
    let nearest = (a - eye).dot(fwd).min((b - eye).dot(fwd)) - r;
    // The rule holds a droplet this far in front of the body's surface.
    let clear = nearest - SIPHON_BODY_CLEARANCE;
    let mut held = 0;
    let mut unheld = 0;
    let mut first_chest = None;
    for _ in 0..90 {
        h.tick();
        for (_, truly, drawn, scale, age) in h.droplets() {
            if first_chest.is_none() {
                first_chest = Some(truly);
            }
            let (dt, dd) = ((truly - eye).dot(fwd), (drawn - eye).dot(fwd));
            let ray = (truly - eye).normalize();
            assert!(
                ray.cross((drawn - eye).normalize()).length() < 1e-4,
                "off its view ray"
            );
            assert!(
                dd <= nearest + 1e-4,
                "drawn at depth {dd} behind the body {nearest}"
            );
            if dt <= clear {
                assert!(drawn.distance(truly) < 1e-4, "a near droplet was moved");
                unheld += 1;
            } else {
                held += 1;
            }
            // Its apparent size where it truly is: the bench's radius keys
            // 0.03 / 0.11 / 0.02 at life 0 / 0.25 / 1, drawn as a diameter.
            let t = age / 0.5;
            let radius = if t < 0.25 {
                0.03 + (0.11 - 0.03) * t / 0.25
            } else {
                0.11 + (0.02 - 0.11) * (t - 0.25) / 0.75
            };
            let true_size = 2.0 * radius;
            assert!(
                (scale / dd - true_size / dt).abs() < 1e-4,
                "apparent size {} vs {}",
                scale / dd,
                true_size / dt
            );
        }
    }
    assert!(held > 100, "held {held}");
    let chest = VICTIM_POS + Vec3::Y * if pet { PET_CHEST_ABOVE } else { CHEST_ABOVE };
    assert!(
        first_chest.unwrap().distance(chest) < 0.1,
        "the spray is at the {} chest",
        if pet { "pet's" } else { "victim's" }
    );
    (held, unheld)
}

/// Both branches of the hold, over the four cameras: held droplets, and
/// droplets already in front of the body drawn where they are.
fn assert_held_in_front_from_four_sides(pet: bool) {
    let mut unheld = 0;
    for yaw in [0.0, 1.3, 3.0, 4.4] {
        unheld += assert_held_in_front(pet, yaw).1;
    }
    assert!(unheld > 50, "unheld {unheld}");
}

#[test]
fn every_droplet_is_drawn_in_front_of_its_victims_body() {
    assert_held_in_front_from_four_sides(false);
}

#[test]
fn a_pets_spray_is_at_its_chest_and_drawn_in_front_of_its_body() {
    assert_held_in_front_from_four_sides(true);
}

/// The hold follows its victim: a victim that runs at the camera after the
/// spray fires still has every droplet drawn in front of it.
#[test]
fn the_hold_follows_a_victim_running_at_the_camera() {
    let mut h = Harness::new(false);
    let cam = match_camera(VICTIM_POS, 0.0);
    h.camera(cam);
    h.wound();
    h.land(AbilityType::Hemorrhage);
    let (eye, fwd) = (cam.translation, cam.forward().as_vec3());
    let toward = Vec3::new(fwd.x, 0.0, fwd.z).normalize() * -1.0;
    let victim = h.victim;
    let mut checked = 0;
    for _ in 0..80 {
        if h.count::<BleedSpray>() + h.count::<BleedDroplet>() > 0 {
            let mut tf = h.app.world_mut().get_mut::<Transform>(victim).unwrap();
            tf.translation += toward * 0.07;
        }
        h.tick();
        let at = h.app.world().get::<Transform>(victim).unwrap().translation;
        let (a, b, r) = body_capsule(at, false);
        let nearest = (a - eye).dot(fwd).min((b - eye).dot(fwd)) - r;
        for (_, _, drawn, ..) in h.droplets() {
            assert!((drawn - eye).dot(fwd) <= nearest + 1e-4);
            checked += 1;
        }
    }
    assert!(checked > 1000, "{checked}");
}

// ── Lifecycle beside other auras ─────────────────────────────────────────────

/// With Rupture, Mortal Strike's wound and a poison on the victim, only the
/// Rogue's own Hemorrhage wound is the landing: without it, nothing sprays.
#[test]
fn only_the_wound_is_the_landing_among_the_victims_auras() {
    let mut h = Harness::new(false);
    h.other_auras();
    h.land(AbilityType::Hemorrhage);
    let (seen, _) = h.play(1.0);
    assert_eq!(seen, 0, "Rupture is not Hemorrhage's landing");

    let mut h = Harness::new(false);
    h.other_auras();
    h.wound();
    h.land(AbilityType::Hemorrhage);
    let (seen, _) = h.play(1.0);
    assert_eq!(seen, DROPLETS);
}

/// What the victim meets after the hit — killed before the blade arrives,
/// or killed, polymorphed, stealthed or despawned mid-spray — beside its
/// other auras: before contact no blood; mid-spray the spray plays out its
/// track, and every spray and droplet retires.
#[test]
fn the_spray_rides_out_its_victims_fate() {
    // Killed before contact: no blood from a corpse.
    let mut h = Harness::new(false);
    h.other_auras();
    h.wound();
    h.land(AbilityType::Hemorrhage);
    h.ticks(5);
    h.kill_victim();
    let (seen, _) = h.play(1.0);
    assert_eq!(seen, 0, "a corpse sprays nothing");
    assert_eq!(h.bleed(), 0);

    type Fate = fn(&mut Harness);
    let fates: [(&str, Fate); 4] = [
        ("killed", |h| h.kill_victim()),
        ("polymorphed", |h| {
            let (rogue, victim) = (h.rogue, h.victim);
            let sheep = aura_of(AbilityType::Polymorph, victim, rogue);
            h.auras().auras.push(sheep);
        }),
        ("stealthed", |h| {
            let victim = h.victim;
            h.app
                .world_mut()
                .get_mut::<Combatant>(victim)
                .unwrap()
                .stealthed = true;
        }),
        ("despawned", |h| {
            let victim = h.victim;
            h.app.world_mut().entity_mut(victim).despawn();
        }),
    ];
    for (name, fate) in fates {
        for pet in [false, true] {
            let mut h = Harness::new(pet);
            h.camera(match_camera(VICTIM_POS, 1.0));
            h.other_auras();
            h.wound();
            h.land(AbilityType::Hemorrhage);
            // Into the spray: it fired at contact.
            h.run_to_spray();
            h.ticks(10);
            fate(&mut h);
            let (seen, _) = h.play(1.0);
            assert!(seen > 0, "{name} (pet {pet}): the spray plays on");
            assert_eq!(h.bleed(), 0, "{name} (pet {pet}): every piece retired");
        }
    }
}

/// The match ending mid-spray takes every piece with it: the held spray, the
/// spray and its droplets are all the match's to clean up.
#[test]
fn the_match_ending_mid_spray_takes_every_piece() {
    for pet in [false, true] {
        let mut h = Harness::new(pet);
        h.wound();
        h.land(AbilityType::Hemorrhage);
        h.ticks(2);
        // A second hit still held, beside the first one spraying.
        h.run_to_spray();
        h.land(AbilityType::Hemorrhage);
        h.ticks(5);
        assert!(h.count::<BleedSprayPending>() == 1, "a held spray");
        assert!(h.count::<BleedSpray>() == 1, "a live spray");
        assert!(h.count::<BleedDroplet>() > 10, "droplets in flight");
        let mut q = h.app.world_mut().query_filtered::<(), (
            Or<(
                With<BleedSprayPending>,
                With<BleedSpray>,
                With<BleedDroplet>,
            )>,
            Without<PlayMatchEntity>,
        )>();
        assert_eq!(q.iter(h.app.world()).count(), 0, "every piece is tagged");
        h.app
            .world_mut()
            .run_system_once(cleanup_play_match)
            .unwrap();
        h.tick();
        assert_eq!(h.bleed(), 0, "pet {pet}: the match's cleanup took them");
    }
}

/// A long fight retires every spray: forty hits a second apart, every one a
/// whole spray, and nothing left a second after the last.
#[test]
fn a_long_fight_retires_every_spray() {
    let mut h = Harness::new(false);
    h.camera(match_camera(VICTIM_POS, 2.0));
    let mut seen = std::collections::HashSet::new();
    let mut most = 0;
    for _ in 0..40 {
        h.wound();
        h.land(AbilityType::Hemorrhage);
        for _ in 0..100 {
            h.tick();
            let mut q = h
                .app
                .world_mut()
                .query_filtered::<Entity, With<BleedDroplet>>();
            seen.extend(q.iter(h.app.world()));
            most = most.max(h.bleed());
        }
    }
    assert_eq!(seen.len(), 40 * DROPLETS);
    // No more than one spray's worth is ever live at once.
    assert!(most <= DROPLETS + 1, "{most}");
    h.ticks(100);
    assert_eq!(h.bleed(), 0);
}
