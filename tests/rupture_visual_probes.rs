//! Probes for Rupture's visual (AS-365): the backhand apply stroke, the
//! finisher hand flash riding the striking hand, the bleed spray fired at
//! contact only on a landing, and the bleed drips (the last is pinned by the
//! lands-silently audit's DoT router test).
//!
//! Everything is sampled through the real graphical stack — the router
//! (`consume_instant_ability_signals`), the swing and body lean, the flash
//! placement, the spray and the shared client-particle animator — and asserted
//! on WORLD positions, the spawned materials and meshes, never on stored
//! fields alone (`test-geometry-not-bookkeeping`).
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU.

use std::time::Duration;

use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    ActiveAuras, AuraPending, ClientParticle, Combatant, InstantAbilityFired, SwingStyle,
    VisualBody, WeaponHand, WeaponKind, WeaponSocket,
};
use arenasim::states::play_match::{
    animate_body_lean, animate_client_particles, animate_weapon_swings, billboard_bleed_droplets,
    consume_instant_ability_signals, fire_bleed_sprays, update_bleed_sprays,
    update_finisher_hand_flashes, BleedDroplet, BleedSpray, BleedSprayPending, FinisherFlashPiece,
    FinisherHandFlash, BACKHAND_CONTACT_SECS,
};
use arenasim::CharacterClass;

/// 10 ms frames: fine enough to sample the stroke's 110 ms phases.
const TICK: Duration = Duration::from_millis(10);

/// The Rogue stands at the origin, its victim 2 yd ahead on +Z (the Rogue's
/// standoff), so the aim frame is the world frame: +X the weapon side.
const ROGUE_POS: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const VICTIM_POS: Vec3 = Vec3::new(0.0, 1.0, 2.0);
/// The victim's chest, where the spray fires (`IMPACT_CHEST_Y` above it).
const VICTIM_CHEST: Vec3 = Vec3::new(0.0, 1.55, 2.0);

/// The shipped dagger mounts (`weapon_mount` in `play_match/mod.rs`): the grip
/// 0.7 yd out on its hand's side, pitched 0.9 rad forward and rolled a quarter
/// turn, at 0.8 scale.
fn dagger_mount(hand: WeaponHand) -> Transform {
    let side = if hand == WeaponHand::Off { -1.0 } else { 1.0 };
    Transform::from_xyz(0.7 * side, -0.05, 0.3)
        .with_rotation(
            Quat::from_rotation_x(0.9) * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        )
        .with_scale(Vec3::splat(0.8))
}

struct Harness {
    app: App,
    rogue: Entity,
    victim: Entity,
    body: Entity,
    main: Entity,
    off: Entity,
}

impl Harness {
    fn new() -> Self {
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
        app.insert_resource(AbilityDefinitions::default());
        // The registration order of `states/mod.rs`: the router claims the
        // stroke, the swing and lean write the rig, the flash lands on it, and
        // the spray fires, emits, flies and faces the camera.
        app.add_systems(
            Update,
            (
                consume_instant_ability_signals,
                animate_weapon_swings,
                animate_body_lean,
                update_finisher_hand_flashes,
                fire_bleed_sprays,
                update_bleed_sprays,
                animate_client_particles,
                billboard_bleed_droplets,
            )
                .chain(),
        );

        let rogue = app
            .world_mut()
            .spawn((
                Transform::from_translation(ROGUE_POS),
                Combatant::new(0, 0, CharacterClass::Rogue),
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
        let victim = app
            .world_mut()
            .spawn((
                Transform::from_translation(VICTIM_POS),
                Combatant::new(1, 0, CharacterClass::Warrior),
                ActiveAuras { auras: Vec::new() },
            ))
            .id();
        Harness {
            app,
            rogue,
            victim,
            body,
            main,
            off,
        }
    }

    fn camera(&mut self, at: Vec3) -> Transform {
        let cam = Transform::from_translation(at).looking_at(VICTIM_POS, Vec3::Y);
        self.app.world_mut().spawn((Camera3d::default(), cam));
        cam
    }

    fn fire(&mut self) {
        let (rogue, victim) = (self.rogue, self.victim);
        self.app.world_mut().spawn(InstantAbilityFired {
            caster: rogue,
            target: Some(victim),
            ability: AbilityType::Rupture,
            is_crit: false,
        });
    }

    /// Land Rupture on the victim from `caster`, as `apply_pending_auras`
    /// would: the aura the ability's own config builds.
    fn land_rupture_from(&mut self, caster: Entity) {
        let victim = self.victim;
        let defs = AbilityDefinitions::default();
        let aura =
            AuraPending::from_ability(victim, caster, defs.get(&AbilityType::Rupture).unwrap())
                .expect("Rupture applies an aura")
                .aura;
        self.app
            .world_mut()
            .get_mut::<ActiveAuras>(victim)
            .unwrap()
            .auras
            .push(aura);
    }

    fn tick(&mut self) {
        self.app.update();
    }

    fn socket(&self, e: Entity) -> &WeaponSocket {
        self.app.world().get::<WeaponSocket>(e).unwrap()
    }

    fn world_of(&self, e: Entity) -> Vec3 {
        self.app
            .world()
            .get::<GlobalTransform>(e)
            .unwrap()
            .translation()
    }

    /// `(stroke seconds, main-hand grip in world space)` for every frame of
    /// the stroke, ending once the socket is back on `Auto`.
    fn sample_grip(&mut self) -> Vec<(f32, Vec3)> {
        let mut out = Vec::new();
        for _ in 0..200 {
            self.tick();
            let Some(t) = self.socket(self.main).release_t else {
                if !out.is_empty() {
                    break;
                }
                continue;
            };
            out.push((t, self.world_of(self.main)));
        }
        out
    }

    fn pieces(&mut self) -> Vec<(Entity, FinisherFlashPiece)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(Entity, &FinisherFlashPiece)>();
        q.iter(self.app.world()).map(|(e, p)| (e, *p)).collect()
    }

    fn piece(&mut self, which: FinisherFlashPiece) -> Entity {
        self.pieces()
            .into_iter()
            .find(|(_, p)| *p == which)
            .map(|(e, _)| e)
            .unwrap_or_else(|| panic!("no {which:?}"))
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

    /// Every vertex position of `e`'s mesh.
    fn vertices(&self, e: Entity) -> Vec<Vec3> {
        let handle = &self.app.world().get::<Mesh3d>(e).unwrap().0;
        let mesh = self
            .app
            .world()
            .resource::<Assets<Mesh>>()
            .get(handle)
            .unwrap();
        match mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() {
            VertexAttributeValues::Float32x3(v) => v.iter().map(|p| Vec3::from(*p)).collect(),
            other => panic!("unexpected positions {other:?}"),
        }
    }

    fn count<C: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query_filtered::<(), With<C>>();
        q.iter(self.app.world()).count()
    }

    /// Ticks until the stroke clock reads at least `t`.
    fn run_to(&mut self, t: f32) {
        for _ in 0..400 {
            if self.socket(self.main).release_t.is_some_and(|r| r >= t) {
                return;
            }
            self.tick();
        }
        panic!("the stroke never reached {t}");
    }

    fn kill_victim(&mut self) {
        let victim = self.victim;
        self.app
            .world_mut()
            .get_mut::<Combatant>(victim)
            .unwrap()
            .current_health = 0.0;
    }
}

fn at(samples: &[(f32, Vec3)], t: f32) -> Vec3 {
    samples
        .iter()
        .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))
        .unwrap()
        .1
}

// ── The stroke ───────────────────────────────────────────────────────────────

#[test]
fn rupture_claims_the_backhand_and_consumes_its_marker() {
    let mut h = Harness::new();
    h.fire();
    h.tick();
    assert_eq!(h.socket(h.main).swing_style, SwingStyle::Backhand);
    assert_eq!(
        h.socket(h.off).swing_style,
        SwingStyle::Auto,
        "a styled stroke is the main hand's alone"
    );
    assert_eq!(h.count::<InstantAbilityFired>(), 0);
}

#[test]
fn the_backhand_contacts_mid_sweep_and_lasts_point_nine_seconds() {
    assert!((SwingStyle::Backhand.impact_at() - 0.33).abs() < 1e-6);
    assert!((BACKHAND_CONTACT_SECS - 0.33).abs() < 1e-6);
    assert!((SwingStyle::Backhand.stroke_secs() - 0.9).abs() < 1e-6);
}

/// The whole stroke in world space: wound across the body on the off side and
/// up at the shoulder, centred on the victim's chest line at contact, wide on
/// the weapon side at the follow-through, and home at the end.
#[test]
fn the_backhand_sweeps_from_the_off_shoulder_through_the_chest_to_the_weapon_side() {
    let mut h = Harness::new();
    h.fire();
    let samples = h.sample_grip();
    let rest = ROGUE_POS + dagger_mount(WeaponHand::Main).translation;
    let start = samples.first().unwrap().1;
    assert!(start.distance(rest) < 0.02, "it starts at rest: {start}");

    let wound = at(&samples, 0.22);
    assert!(wound.x < -0.5, "wound across the body: {wound}");
    assert!(wound.y > ROGUE_POS.y + 0.55, "up at the shoulder: {wound}");

    let contact = at(&samples, 0.33);
    assert!(
        contact.x.abs() < 0.1,
        "centred on the chest line: {contact}"
    );
    assert!(
        contact.z > ROGUE_POS.z + 1.3,
        "out at the victim: {contact}"
    );
    assert!(
        (contact.y - VICTIM_CHEST.y).abs() < 0.1,
        "at chest height: {contact}"
    );

    let follow = at(&samples, 0.44);
    assert!(follow.x > 1.1, "wide on the weapon side: {follow}");
    let held = at(&samples, 0.5);
    assert!(held.distance(follow) < 0.01, "held at the follow-through");

    let end = samples.last().unwrap().1;
    assert!(end.distance(rest) < 0.02, "home at the end: {end}");
}

/// The sweep moves the grip OUTWARD — off side to weapon side — on every frame
/// of it, never doubling back.
#[test]
fn the_sweep_runs_one_way_across_the_body() {
    let mut h = Harness::new();
    h.fire();
    let samples = h.sample_grip();
    let sweep: Vec<f32> = samples
        .iter()
        .filter(|(t, _)| (0.23..=0.43).contains(t))
        .map(|(_, p)| p.x)
        .collect();
    assert!(sweep.len() > 15, "{}", sweep.len());
    for pair in sweep.windows(2) {
        assert!(pair[1] > pair[0], "doubled back: {sweep:?}");
    }
}

/// What makes it read apart from the Rogue's other strokes: the backhand's
/// grip crosses the body from the off side to well past the weapon-side rest;
/// no other Rogue stroke's grip ever reaches the off side at all.
#[test]
fn only_the_backhand_crosses_the_body() {
    let lateral_span = |ability: AbilityType| {
        let mut h = Harness::new();
        let (rogue, victim) = (h.rogue, h.victim);
        h.app.world_mut().spawn(InstantAbilityFired {
            caster: rogue,
            target: Some(victim),
            ability,
            is_crit: false,
        });
        let xs: Vec<f32> = h.sample_grip().iter().map(|(_, p)| p.x).collect();
        (
            xs.iter().copied().fold(f32::INFINITY, f32::min),
            xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        )
    };
    let (min, max) = lateral_span(AbilityType::Rupture);
    assert!(min < -0.5 && max > 1.1, "backhand {min}..{max}");
    for other in [
        AbilityType::Ambush,
        AbilityType::KidneyShot,
        AbilityType::CheapShot,
        AbilityType::SinisterStrike,
    ] {
        let (min, _) = lateral_span(other);
        assert!(min > 0.0, "{other:?} reaches the off side ({min})");
    }
}

/// The weapon never dips into the capsule (radius 0.5) the stroke crosses in
/// front of: the windup bows forward round it.
#[test]
fn the_grip_stays_clear_of_the_body() {
    let mut h = Harness::new();
    h.fire();
    for (t, p) in h.sample_grip() {
        let r = (p - ROGUE_POS).with_y(0.0).length();
        assert!(r > 0.6, "inside the body at {t}: {p} ({r})");
    }
}

/// The blade turns with the sweep: across the body when wound, into the
/// victim at contact, trailing out on the weapon side after, and laid nearly
/// level throughout.
#[test]
fn the_blade_turns_through_the_victim() {
    let mut h = Harness::new();
    h.fire();
    let mut blades = Vec::new();
    for _ in 0..120 {
        h.tick();
        let Some(t) = h.socket(h.main).release_t else {
            continue;
        };
        let rot = h
            .app
            .world()
            .get::<GlobalTransform>(h.main)
            .unwrap()
            .compute_transform()
            .rotation;
        // The model's blade runs along its local +Y.
        blades.push((t, rot * Vec3::Y));
    }
    let blade = |t: f32| {
        blades
            .iter()
            .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))
            .unwrap()
            .1
    };
    assert!(blade(0.22).x < -0.8, "across the body: {}", blade(0.22));
    let contact = blade(0.33);
    assert!(contact.z > 0.95, "into the victim: {contact}");
    assert!(blade(0.44).x > 0.8, "trailing out: {}", blade(0.44));
    for t in [0.22, 0.33, 0.44] {
        assert!(blade(t).y.abs() < 0.2, "level at {t}: {}", blade(t));
    }
}

/// The body steps back as the stroke winds and into the sweep, then home —
/// and never turns.
#[test]
fn the_body_steps_into_the_sweep_without_turning() {
    let mut h = Harness::new();
    h.fire();
    let mut steps = Vec::new();
    for _ in 0..120 {
        h.tick();
        let Some(t) = h.socket(h.main).release_t else {
            continue;
        };
        let body = *h.app.world().get::<Transform>(h.body).unwrap();
        assert!(
            body.rotation.angle_between(Quat::IDENTITY) < 1e-5,
            "the body turned at {t}"
        );
        steps.push((t, body.translation.z));
    }
    let step = |t: f32| {
        at(
            &steps
                .iter()
                .map(|(t, z)| (*t, Vec3::Z * *z))
                .collect::<Vec<_>>(),
            t,
        )
        .z
    };
    assert!(
        (step(0.22) + 0.06).abs() < 0.01,
        "back at the wind: {}",
        step(0.22)
    );
    assert!(
        step(0.33) > 0.15,
        "into the sweep at contact: {}",
        step(0.33)
    );
    assert!((step(0.5) - 0.28).abs() < 0.01, "held in: {}", step(0.5));
}

// ── The finisher hand flash ─────────────────────────────────────────────────

/// Every piece rides its hand through the stroke: the glow and ring ON the
/// striking hand's grip, the puffs just off it, the off glow on the off hand.
#[test]
fn the_flash_rides_the_striking_hand() {
    let mut h = Harness::new();
    h.camera(Vec3::new(8.0, 10.0, -6.0));
    h.fire();
    let mut checked = 0;
    for _ in 0..80 {
        h.tick();
        let Some(t) = h.socket(h.main).release_t else {
            continue;
        };
        if t < 0.05 || t > 0.85 {
            continue;
        }
        let hand = h.world_of(h.main);
        let off = h.world_of(h.off);
        for (e, piece) in h.pieces() {
            let p = h.world_of(e);
            match piece {
                FinisherFlashPiece::Glow | FinisherFlashPiece::Ring => {
                    assert!(p.distance(hand) < 1e-3, "{piece:?} off the hand at {t}")
                }
                FinisherFlashPiece::Puff(_) => {
                    assert!(p.distance(hand) < 0.3, "{piece:?} strayed at {t}")
                }
                FinisherFlashPiece::OffGlow => {
                    assert!(p.distance(off) < 1e-3, "off glow off the off hand at {t}")
                }
            }
        }
        checked += 1;
    }
    assert!(checked > 50, "{checked}");
}

/// The glow's drawn colour, blend, sprite and size, and its strength full at
/// the stroke's contact.
#[test]
fn the_glow_is_the_benchs_red_additive_quad_full_at_contact() {
    let mut h = Harness::new();
    h.camera(Vec3::new(8.0, 10.0, -6.0));
    h.fire();
    h.tick();
    let glow = h.piece(FinisherFlashPiece::Glow);
    let off = h.piece(FinisherFlashPiece::OffGlow);
    h.run_to(0.33);
    let t = h.socket(h.main).release_t.unwrap();
    let m = h.material(glow);
    let c = m.base_color.to_srgba();
    assert!((c.red - 1.0).abs() < 1e-5);
    assert!((c.green - 30.0 / 255.0).abs() < 1e-5);
    assert!((c.blue - 10.0 / 255.0).abs() < 1e-5);
    assert!(
        c.alpha > 0.53 && c.alpha <= 0.55 + 1e-6,
        "full at contact ({t}): {}",
        c.alpha
    );
    assert_eq!(m.alpha_mode, AlphaMode::Add);
    assert!(m.unlit);
    assert!(
        m.base_color_texture.is_some(),
        "a soft sprite, not a flat card"
    );
    // A unit quad, scaled to the 0.9 yd glow.
    let v = h.vertices(glow);
    let extent = v.iter().fold(Vec3::ZERO, |a, p| a.max(p.abs()));
    assert!((extent.x - 0.5).abs() < 1e-6 && (extent.y - 0.5).abs() < 1e-6);
    let scale = h.app.world().get::<Transform>(glow).unwrap().scale;
    assert!((scale.x - 0.9).abs() < 1e-6, "{scale}");
    // The off hand's: smaller and dimmer.
    let scale = h.app.world().get::<Transform>(off).unwrap().scale;
    assert!((scale.x - 0.6).abs() < 1e-6, "{scale}");
    let a = h.material(off).base_color.alpha();
    assert!(a > 0.33 && a <= 0.35 + 1e-6, "{a}");
}

/// The ring: a thin flat annulus round the hand, yellow at the start and red
/// by 0.45 s, growing from 0.22 to 0.41 yd across.
#[test]
fn the_ring_is_a_flat_yellow_to_red_annulus() {
    let mut h = Harness::new();
    h.camera(Vec3::new(-5.0, 12.0, 9.0));
    h.fire();
    h.tick();
    h.tick();
    let ring = h.piece(FinisherFlashPiece::Ring);
    let radii: Vec<f32> = h
        .vertices(ring)
        .iter()
        .map(|p| p.truncate().length())
        .collect();
    let (lo, hi) = radii.iter().fold((f32::INFINITY, 0.0f32), |(lo, hi), r| {
        (lo.min(*r), hi.max(*r))
    });
    assert!(
        (lo - 0.85).abs() < 1e-4 && (hi - 1.0).abs() < 1e-4,
        "{lo}..{hi}"
    );
    let m = h.material(ring);
    assert_eq!(m.alpha_mode, AlphaMode::Add);
    assert!(m.unlit);
    assert!(m.base_color_texture.is_none(), "a solid line");
    let c = m.base_color.to_srgba();
    assert!(
        (c.red - 1.0).abs() < 0.01 && c.green > 0.8 && c.blue < 0.01,
        "yellow: {c:?}"
    );
    let tf = *h.app.world().get::<Transform>(ring).unwrap();
    assert!(
        (tf.rotation * Vec3::Z).dot(Vec3::Y).abs() > 0.999,
        "lies flat"
    );
    assert!(tf.scale.x < 0.12 && tf.scale.x > 0.109, "{}", tf.scale.x);
    h.run_to(0.5);
    let c = h.material(ring).base_color.to_srgba();
    assert!(
        (c.red - 250.0 / 255.0).abs() < 1e-5 && (c.green - 5.0 / 255.0).abs() < 1e-5,
        "red: {c:?}"
    );
    let tf = *h.app.world().get::<Transform>(ring).unwrap();
    assert!(
        (tf.rotation * Vec3::Z).dot(Vec3::Y).abs() > 0.999,
        "still flat"
    );
}

/// The puffs: six soft red-orange sprites, drifting back from the victim.
#[test]
fn the_puffs_are_six_red_sprites_drifting_back() {
    let mut h = Harness::new();
    h.camera(Vec3::new(8.0, 10.0, -6.0));
    h.fire();
    h.run_to(0.3);
    let puffs: Vec<Entity> = h
        .pieces()
        .into_iter()
        .filter(|(_, p)| matches!(p, FinisherFlashPiece::Puff(_)))
        .map(|(e, _)| e)
        .collect();
    assert_eq!(puffs.len(), 6);
    let hand = h.world_of(h.main);
    for e in puffs {
        let m = h.material(e);
        let c = m.base_color.to_srgba();
        assert!((c.red - 1.0).abs() < 1e-5 && (c.green - 40.0 / 255.0).abs() < 1e-5);
        assert_eq!(m.alpha_mode, AlphaMode::Add);
        assert!(m.base_color_texture.is_some());
        let p = h.world_of(e);
        // Never ahead of the hand toward the victim (+Z).
        assert!(p.z <= hand.z + 1e-4, "a puff drifts toward the victim");
        assert!(h.app.world().get::<Transform>(e).unwrap().scale.x <= 0.28 + 1e-6);
    }
}

/// The glow and puffs face the camera wherever it stands; the ring does not.
#[test]
fn the_flash_faces_the_camera() {
    for cam_at in [Vec3::new(12.0, 8.0, 0.0), Vec3::new(-3.0, 30.0, -20.0)] {
        let mut h = Harness::new();
        let cam = h.camera(cam_at);
        h.fire();
        h.run_to(0.3);
        let mut faced = 0;
        for (e, piece) in h.pieces() {
            let rot = h.app.world().get::<Transform>(e).unwrap().rotation;
            if piece.faces_camera() {
                assert!(
                    (rot * Vec3::Z).dot(cam.rotation * Vec3::Z) > 0.999
                        && (rot * Vec3::Y).dot(cam.rotation * Vec3::Y) > 0.999,
                    "{piece:?} does not face the camera at {cam_at}"
                );
                faced += 1;
            }
        }
        assert_eq!(faced, 8, "glow, six puffs and the off glow");
    }
}

#[test]
fn the_flash_is_out_with_the_stroke() {
    let mut h = Harness::new();
    h.fire();
    h.tick();
    assert_eq!(h.count::<FinisherHandFlash>(), 1);
    // The glow, the ring, six puffs and the off glow.
    assert_eq!(h.count::<FinisherFlashPiece>(), 9);
    for _ in 0..95 {
        h.tick();
    }
    assert_eq!(h.count::<FinisherHandFlash>(), 0);
    assert_eq!(h.count::<FinisherFlashPiece>(), 0, "its pieces go with it");
}

/// A Rogue killed mid-stroke takes its flash with it: nothing glows on a
/// corpse.
#[test]
fn a_dead_rogue_drops_its_flash() {
    let mut h = Harness::new();
    h.fire();
    h.run_to(0.2);
    assert_eq!(h.count::<FinisherHandFlash>(), 1);
    let rogue = h.rogue;
    h.app
        .world_mut()
        .get_mut::<Combatant>(rogue)
        .unwrap()
        .current_health = 0.0;
    h.tick();
    h.tick();
    assert_eq!(h.count::<FinisherHandFlash>(), 0);
    assert_eq!(h.count::<FinisherFlashPiece>(), 0);
}

// ── The bleed spray ─────────────────────────────────────────────────────────

/// Plays the stroke through with the victim's state set up by `stage`, and
/// returns `(droplets alive just before contact, distinct droplets seen in
/// all, the droplets' positions just after contact)`.
fn spray(stage: impl FnOnce(&mut Harness)) -> (usize, usize, Vec<Vec3>) {
    let mut h = Harness::new();
    h.camera(Vec3::new(8.0, 10.0, -6.0));
    stage(&mut h);
    h.fire();
    h.run_to(0.31);
    let before = h.count::<BleedDroplet>();
    h.run_to(0.36);
    let mut q = h
        .app
        .world_mut()
        .query_filtered::<&GlobalTransform, With<BleedDroplet>>();
    let early: Vec<Vec3> = q.iter(h.app.world()).map(|g| g.translation()).collect();
    // A droplet lives 0.5 s, so every one is alive on some 10 ms frame.
    let mut seen = std::collections::HashSet::new();
    for _ in 0..80 {
        let mut q = h
            .app
            .world_mut()
            .query_filtered::<Entity, With<BleedDroplet>>();
        seen.extend(q.iter(h.app.world()));
        h.tick();
    }
    (before, seen.len(), early)
}

#[test]
fn a_landed_rupture_sprays_at_contact_from_the_chest() {
    let (before, emitted, early) = spray(|h| {
        let rogue = h.rogue;
        h.land_rupture_from(rogue);
    });
    assert_eq!(before, 0, "nothing before the blade arrives");
    assert!(!early.is_empty(), "the spray fires at contact");
    for p in &early {
        assert!(p.distance(VICTIM_CHEST) < 0.2, "{p} is not at the chest");
    }
    // The rate track's 274.95 droplets: 274 whole ones.
    assert_eq!(emitted, 274);
}

/// A dodged or parried Rupture lands no bleed, so it draws no blood — though
/// the stroke and the hand flash still play.
#[test]
fn a_rupture_that_landed_nothing_draws_no_blood() {
    let mut h = Harness::new();
    h.fire();
    h.run_to(0.6);
    assert_eq!(h.count::<BleedSpray>(), 0);
    assert_eq!(h.count::<BleedDroplet>(), 0);
    assert_eq!(
        h.count::<BleedSprayPending>(),
        0,
        "the held spray is dropped"
    );
    assert_eq!(h.socket(h.main).swing_style, SwingStyle::Backhand);
    assert_eq!(h.count::<FinisherHandFlash>(), 1);
}

/// Another Rogue's Rupture on the victim is not this one's landing.
#[test]
fn another_rogues_rupture_is_not_this_ones_landing() {
    let (_, emitted, _) = spray(|h| {
        let other = h
            .app
            .world_mut()
            .spawn(Combatant::new(0, 1, CharacterClass::Rogue))
            .id();
        h.land_rupture_from(other);
    });
    assert_eq!(emitted, 0);
}

/// A victim dead by the time the blade arrives sprays nothing — no blood from
/// a corpse — even with the bleed still on it.
#[test]
fn a_corpse_sprays_nothing() {
    let (_, emitted, _) = spray(|h| {
        let rogue = h.rogue;
        h.land_rupture_from(rogue);
        h.kill_victim();
    });
    assert_eq!(emitted, 0);
}

/// The droplets as drawn: a unit quad at the bench's radius keys (0.06 yd
/// across at birth), alpha-BLENDED unlit deep red with the solid-disc sprite,
/// facing the camera.
#[test]
fn the_droplets_are_opaque_deep_red_discs_facing_the_camera() {
    let mut h = Harness::new();
    let cam = h.camera(Vec3::new(-6.0, 14.0, 12.0));
    let rogue = h.rogue;
    h.land_rupture_from(rogue);
    h.fire();
    h.run_to(0.345);
    let mut q = h
        .app
        .world_mut()
        .query_filtered::<(Entity, &ClientParticle), With<BleedDroplet>>();
    let droplets: Vec<(Entity, f32)> = q.iter(h.app.world()).map(|(e, p)| (e, p.age)).collect();
    assert!(droplets.len() > 10, "{}", droplets.len());
    let mut youngest = None;
    for (e, age) in &droplets {
        let tf = *h.app.world().get::<Transform>(*e).unwrap();
        assert!(
            (tf.rotation * Vec3::Z).dot(cam.rotation * Vec3::Z) > 0.999,
            "a droplet does not face the camera"
        );
        if youngest.is_none_or(|(_, a)| *age < a) {
            youngest = Some((*e, *age));
        }
    }
    let (e, _) = youngest.unwrap();
    let m = h.material(e);
    assert_eq!(m.alpha_mode, AlphaMode::Blend, "opaque blood, not a glow");
    assert!(m.unlit);
    assert!(m.base_color_texture.is_some());
    let c = m.base_color.to_srgba();
    assert!(
        c.red > 0.5 && c.red < 0.6 && c.green < 0.04 && c.blue < 0.04,
        "{c:?}"
    );
    assert!(c.alpha > 0.9, "{c:?}");
    let v = h.vertices(e);
    let extent = v.iter().fold(Vec3::ZERO, |a, p| a.max(p.abs()));
    assert!((extent.x - 0.5).abs() < 1e-6 && (extent.y - 0.5).abs() < 1e-6);
    // Born at the bench's 0.03 yd radius: 0.06 across, growing.
    let scale = h.app.world().get::<Transform>(e).unwrap().scale.x;
    assert!((0.059..0.12).contains(&scale), "{scale}");
}

/// The droplets fall: gravity pulls every one down from where it would have
/// flown.
#[test]
fn the_droplets_arc_down() {
    let mut h = Harness::new();
    let rogue = h.rogue;
    h.land_rupture_from(rogue);
    h.fire();
    h.run_to(0.34);
    let mut q = h
        .app
        .world_mut()
        .query_filtered::<(Entity, &ClientParticle), With<BleedDroplet>>();
    let first: Vec<(Entity, f32)> = q
        .iter(h.app.world())
        .map(|(e, p)| (e, p.velocity.y))
        .collect();
    assert!(!first.is_empty());
    for _ in 0..10 {
        h.tick();
    }
    for (e, vy) in first {
        if let Some(p) = h.app.world().get::<ClientParticle>(e) {
            assert!(p.velocity.y < vy - 0.5, "gravity did not pull it down");
        }
    }
}

/// Another of this Rogue's own auras on the victim (its Kidney Shot) is not
/// Rupture's landing either.
#[test]
fn only_the_bleed_itself_is_the_landing() {
    let (_, emitted, _) = spray(|h| {
        let (rogue, victim) = (h.rogue, h.victim);
        let defs = AbilityDefinitions::default();
        let aura =
            AuraPending::from_ability(victim, rogue, defs.get(&AbilityType::KidneyShot).unwrap())
                .expect("Kidney Shot applies an aura")
                .aura;
        h.app
            .world_mut()
            .get_mut::<ActiveAuras>(victim)
            .unwrap()
            .auras
            .push(aura);
    });
    assert_eq!(emitted, 0);
}
