//! Probes for Repentance's visual (AS-378, `rendering/effects/repentance.rs`
//! and its impact in `paladin_oneshots.rs`): the Holy hand flash on the
//! Paladin, the gold impact at the victim's chest, and the violet head swirl
//! for as long as the incapacitate holds — never Freezing Trap's ice block.
//!
//! The incapacitate is landed the way the class AI lands it — an
//! `AuraPending` from the shipped config through the real
//! `apply_pending_auras` — and ended by the real systems that end it: damage
//! through `process_aura_breaks`, a dispel through `process_dispels`, expiry
//! through `update_auras`, and a killing blow (which leaves the aura on the
//! corpse). WORLD geometry is asserted through `GlobalTransform`, the camera
//! facing under cameras turned to several bearings, and what each piece is
//! drawn with by reading its material back.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU. The visual systems run in the order `states/mod.rs` registers
//! them, after the sim systems that land and end the aura.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::combat::log::CombatLog;
use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::auras::{apply_pending_auras, process_aura_breaks, update_auras};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, AuraApplyOwner, AuraApplyRoute, AuraLandingKind, AuraPending,
    AuraType, CameraController, ClientParticle, Combatant, DamageTakenThisFrame, DispelPending,
    DispelScope, GameRng, HealCastHand, HealCastKind, IceBlockVisual, PaladinOneShot,
    PaladinOneShotKind, RepentanceStar, RepentanceSwirl, VisualBody, FREEZING_TRAP_AURA,
    REPENTANCE_AURA,
};
use arenasim::states::play_match::effects::process_dispels;
use arenasim::states::play_match::traps::freezing_trap_aura;
use arenasim::states::play_match::{
    animate_client_particles, billboard_hunter_shots, cleanup_ice_blocks,
    consume_cast_ending_signals, oneshot_secs, spawn_instant_cast_hands, spawn_paladin_oneshots,
    star_flash_texture, update_heal_cast_glows, update_paladin_oneshots, update_repentance_swirls,
    REPENTANCE_IMPACT_BODY_REACH, STAR_ORBIT_RADII,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;

/// The head anchor: the bench's 2.25 yd, 1.25 above a combatant's centre.
const HEAD_ABOVE_CENTRE: f32 = 1.25;
/// The chest anchor, 0.45 above the centre.
const CHEST_ABOVE_CENTRE: f32 = 0.45;

struct Harness {
    app: App,
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
        app.insert_resource(AbilityDefinitions::default());
        app.insert_resource(CombatLog::default());
        app.insert_resource(ArenaDampening::default());
        app.insert_resource(GameRng::from_seed(7));
        app.add_systems(
            Update,
            (
                // The sim: land, break, dispel and tick the aura.
                (
                    apply_pending_auras,
                    process_aura_breaks,
                    process_dispels,
                    update_auras,
                )
                    .chain(),
                // The visuals, as `states/mod.rs` orders them.
                (
                    spawn_instant_cast_hands,
                    consume_cast_ending_signals,
                    spawn_paladin_oneshots,
                    update_paladin_oneshots,
                    update_repentance_swirls,
                    update_heal_cast_glows,
                    animate_client_particles,
                    billboard_hunter_shots,
                    cleanup_ice_blocks,
                )
                    .chain(),
            )
                .chain(),
        );
        Harness { app }
    }

    /// A unit as `spawn_combatant` builds it: capsule centre a yard above the
    /// floor, with a `VisualBody` child for the hands to hang off.
    fn unit(&mut self, team: u8, class: CharacterClass, at: Vec3) -> Entity {
        let world = self.app.world_mut();
        let unit = world
            .spawn((
                Combatant::new(team, 0, class),
                Transform::from_translation(at),
            ))
            .id();
        let body = world
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        world.entity_mut(unit).add_child(body);
        unit
    }

    fn camera(&mut self, transform: Transform) -> Quat {
        self.app.world_mut().spawn((Camera3d::default(), transform));
        transform.rotation
    }

    /// Repentance from `paladin` on `victim`, as the class AI lands it.
    fn repent(&mut self, paladin: Entity, victim: Entity) {
        let defs = AbilityDefinitions::default();
        let pending = AuraPending::from_ability(
            victim,
            paladin,
            defs.get_unchecked(&AbilityType::Repentance),
        )
        .expect("Repentance lands an aura");
        self.app.world_mut().spawn(pending);
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn tick_secs(&mut self, secs: f32) {
        self.tick((secs / DT).ceil() as u32);
    }

    fn count<C: Component>(&mut self) -> usize {
        let mut q = self.app.world_mut().query::<&C>();
        q.iter(self.app.world()).count()
    }

    fn swirls(&mut self) -> Vec<(Entity, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&RepentanceSwirl, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(s, g)| (s.owner, g.translation()))
            .collect()
    }

    /// Every star: which it is, its world transform, its material.
    fn stars(&mut self) -> Vec<(RepentanceStar, GlobalTransform, StandardMaterial)> {
        let mut q = self.app.world_mut().query::<(
            &RepentanceStar,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
        )>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .map(|(s, g, m)| (*s, *g, m.0.clone()))
            .collect();
        let materials = self.app.world().resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(s, g, m)| (s, g, materials.get(&m).unwrap().clone()))
            .collect()
    }

    /// The swirl's living sparkles (owned particles; a spent one is shrunk to
    /// nothing until its swirl drops it), with their world transform.
    fn sparkles(&mut self) -> Vec<(GlobalTransform, Vec3, Handle<StandardMaterial>)> {
        let mut q = self.app.world_mut().query::<(
            &ClientParticle,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
        )>();
        q.iter(self.app.world())
            .filter(|(p, _, _)| p.owned && p.age < p.life)
            .map(|(p, g, m)| (*g, p.velocity, m.0.clone()))
            .collect()
    }

    /// Every sparkle entity the swirl holds, spent or not.
    fn sparkle_entities(&mut self) -> usize {
        let mut q = self.app.world_mut().query::<&ClientParticle>();
        q.iter(self.app.world()).filter(|p| p.owned).count()
    }

    /// The impact's particles: world-space ones, with where each will be at
    /// the end of its life.
    fn impact_particles(&mut self) -> Vec<(Vec3, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&ClientParticle, &Transform)>();
        q.iter(self.app.world())
            .filter(|(p, _)| !p.owned)
            .map(|(p, t)| {
                (
                    t.translation,
                    t.translation + p.velocity * (p.life - p.age).max(0.0),
                )
            })
            .collect()
    }

    fn rigs(&mut self) -> Vec<(PaladinOneShotKind, Entity)> {
        let mut q = self.app.world_mut().query::<&PaladinOneShot>();
        q.iter(self.app.world())
            .map(|r| (r.kind, r.bearer))
            .collect()
    }

    fn hands(&mut self) -> Vec<(HealCastKind, Entity)> {
        let mut q = self.app.world_mut().query::<&HealCastHand>();
        q.iter(self.app.world())
            .map(|h| (h.kind, h.caster))
            .collect()
    }

    fn auras(&self, unit: Entity) -> Vec<String> {
        self.app
            .world()
            .get::<ActiveAuras>(unit)
            .map(|a| a.auras.iter().map(|a| a.ability_name.clone()).collect())
            .unwrap_or_default()
    }

    /// Nothing of a swirl is left: no rig, no star, no sparkle.
    fn swirl_gone(&mut self) -> bool {
        self.count::<RepentanceSwirl>() == 0
            && self.count::<RepentanceStar>() == 0
            && self.sparkle_entities() == 0
    }
}

/// A Paladin and its victim, a Mage, 8 yd apart.
fn paladin_and_mage(h: &mut Harness) -> (Entity, Entity, Vec3) {
    let paladin = h.unit(1, CharacterClass::Paladin, Vec3::new(-4.0, 1.0, 0.0));
    let at = Vec3::new(4.0, 1.0, 1.0);
    let mage = h.unit(2, CharacterClass::Mage, at);
    (paladin, mage, at)
}

/// A camera-facing quad's normal (its local +Z) lies along the camera's.
fn faces_camera(g: &GlobalTransform, camera: Quat) -> bool {
    (g.rotation() * Vec3::Z).dot(camera * Vec3::Z) > 0.999
}

/// The match camera (`CameraController`'s placement) at `zoom` and `yaw`,
/// looking at `target`.
fn match_camera(target: Vec3, zoom: f32, yaw: f32) -> Transform {
    let pitch = CameraController::default().pitch;
    let at = target
        + Vec3::new(
            zoom * pitch.sin() * yaw.sin(),
            zoom * pitch.cos(),
            zoom * pitch.sin() * yaw.cos(),
        );
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

/// Where `p` lands on a `size` viewport, pixels, through `camera` and Bevy's
/// default perspective (45° field of view).
fn project(camera: &Transform, p: Vec3, size: Vec2) -> Vec2 {
    let projection = PerspectiveProjection {
        aspect_ratio: size.x / size.y,
        ..default()
    };
    let clip = bevy::render::camera::CameraProjection::get_clip_from_view(&projection)
        * camera.compute_matrix().inverse()
        * p.extend(1.0);
    assert!(clip.w > 0.0, "in front of the camera");
    let ndc = clip.truncate() / clip.w;
    Vec2::new((ndc.x + 1.0) / 2.0 * size.x, (1.0 - ndc.y) / 2.0 * size.y)
}

fn srgb_u8(c: Color) -> [u8; 3] {
    let c = c.to_srgba();
    [c.red, c.green, c.blue].map(|v| (v * 255.0).round() as u8)
}

// ── routing ─────────────────────────────────────────────────────────────────

/// Repentance's incapacitate is routed by its NAME to its own owner and its
/// own landing; an incapacitate by any other name — Freezing Trap's — keeps
/// the ice block. Its hands are the instant Holy flash, off the landing.
#[test]
fn repentance_routes_by_name_to_its_own_owner() {
    let defs = AbilityDefinitions::default();
    let def = defs.get_unchecked(&AbilityType::Repentance);
    assert_eq!(def.name, REPENTANCE_AURA, "the RON name");
    let t = def.applies_aura.as_ref().expect("an aura").aura_type;
    assert_eq!(t, AuraType::Incapacitate);
    assert_eq!(
        AuraApplyRoute::for_instance(t, REPENTANCE_AURA),
        AuraApplyRoute::Owned(AuraApplyOwner::Repentance)
    );
    assert_eq!(
        AuraLandingKind::for_aura(t, REPENTANCE_AURA),
        Some(AuraLandingKind::RepentanceImpact)
    );
    assert_eq!(
        PaladinOneShotKind::for_landing(AuraLandingKind::RepentanceImpact),
        Some(PaladinOneShotKind::RepentanceImpact)
    );
    // Freezing Trap, and an incapacitate with no landing, stay the ice
    // block's.
    let trap = freezing_trap_aura(Entity::from_raw(1));
    assert_eq!(trap.ability_name, FREEZING_TRAP_AURA);
    for name in [FREEZING_TRAP_AURA, "Sap"] {
        assert_eq!(
            AuraApplyRoute::for_instance(AuraType::Incapacitate, name),
            AuraApplyRoute::Owned(AuraApplyOwner::IceBlock),
            "{name}"
        );
        assert_eq!(
            AuraLandingKind::for_aura(AuraType::Incapacitate, name),
            None
        );
    }
    // The name alone is not enough: another type named Repentance routes by
    // its type.
    assert_eq!(
        AuraApplyRoute::for_instance(AuraType::Stun, REPENTANCE_AURA),
        AuraApplyRoute::for_aura(AuraType::Stun)
    );
    // The hands.
    assert_eq!(
        HealCastKind::for_ability(AbilityType::Repentance),
        Some(HealCastKind::HolyFlash)
    );
    assert_eq!(
        AuraLandingKind::RepentanceImpact.cast_ability(),
        Some(AbilityType::Repentance)
    );
    assert!(AuraLandingKind::flashes_hands_for(AbilityType::Repentance));
}

// ── the landing ─────────────────────────────────────────────────────────────

/// The landing: the Holy flash on the PALADIN's hands, and the gold impact
/// on the VICTIM, at its chest, retiring at 0.93 s while the swirl stays.
#[test]
fn the_landing_flashes_the_paladins_hands_and_bursts_gold_at_the_victims_chest() {
    let mut h = Harness::new();
    let (paladin, mage, at) = paladin_and_mage(&mut h);
    h.repent(paladin, mage);
    h.tick(1);
    assert_eq!(h.auras(mage), vec![REPENTANCE_AURA.to_string()]);
    h.tick(1);
    assert_eq!(h.hands(), vec![(HealCastKind::HolyFlash, paladin); 2]);
    assert_eq!(
        h.rigs(),
        vec![(PaladinOneShotKind::RepentanceImpact, mage)],
        "the impact plays on the victim alone"
    );
    h.tick_secs(0.25);
    let particles = h.impact_particles();
    assert!(particles.len() > 15, "{} impact particles", particles.len());
    // With no camera the impact plays exactly at the chest, as the bench.
    let chest = at + Vec3::Y * CHEST_ABOVE_CENTRE;
    for (p, _) in &particles {
        assert!((*p - chest).length() < 0.55, "{p} off the chest {chest}");
    }
    h.tick_secs(oneshot_secs(PaladinOneShotKind::RepentanceImpact) - 0.25);
    assert!(h.rigs().is_empty(), "the impact retires at 0.93 s");
    assert_eq!(h.swirls().len(), 1, "the swirl holds on");
}

/// At the match camera every impact particle stands in front of the
/// victim's body for its whole life — beyond the farthest the capsule
/// reaches toward the camera — while it still reads at the chest: within
/// the bench's reach of the line from the camera through the chest.
#[test]
fn the_impact_stands_in_front_of_the_body_at_the_match_camera() {
    let zoom = CameraController::default().zoom_distance;
    let mut checked = 0;
    for step in 0..4 {
        let yaw = step as f32 * FRAC_PI_2 + 0.3;
        let mut h = Harness::new();
        let (paladin, mage, at) = paladin_and_mage(&mut h);
        let cam = match_camera(at, zoom, yaw);
        h.camera(cam);
        h.repent(paladin, mage);
        h.tick_secs(0.3);
        let chest = at + Vec3::Y * CHEST_ABOVE_CENTRE;
        let toward = (cam.translation - chest).normalize();
        // How far the capsule reaches from the chest toward this camera:
        // its top hemisphere, centred 0.3 above the chest, radius 0.5.
        let reach = 0.3 * toward.y + 0.5;
        assert!(reach <= REPENTANCE_IMPACT_BODY_REACH + 1e-6);
        let particles = h.impact_particles();
        assert!(particles.len() > 15, "{} particles", particles.len());
        for (now, end) in particles {
            for p in [now, end] {
                let along = (p - chest).dot(toward);
                assert!(
                    along > reach,
                    "yaw {yaw}: {p} is {along} toward, behind {reach}"
                );
                let off_line = (p - chest - toward * along).length();
                assert!(
                    off_line < 0.55,
                    "yaw {yaw}: {p} is {off_line} off the chest"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "{checked}");
}

// ── the swirl ───────────────────────────────────────────────────────────────

/// The swirl at the victim's head: twelve stars on the three orbits, facing
/// the camera at every bearing, rolled along their orbit, violet into white
/// down each trail; sparkles falling 3.33 yd/s through the box over the
/// head, facing the camera.
#[test]
fn the_swirl_rings_the_victims_head_while_repentance_holds() {
    for bearing in [0.0_f32, 1.1, 2.6, 4.4] {
        let mut h = Harness::new();
        let (paladin, mage, at) = paladin_and_mage(&mut h);
        let eye = at + Vec3::new(14.0 * bearing.sin(), 9.0, 14.0 * bearing.cos());
        let cam = h.camera(Transform::from_translation(eye).looking_at(at, Vec3::Y));
        h.repent(paladin, mage);
        h.tick_secs(1.0);

        let head = at + Vec3::Y * HEAD_ABOVE_CENTRE;
        let swirls = h.swirls();
        assert_eq!(swirls.len(), 1);
        assert_eq!(swirls[0].0, mage);
        assert!((swirls[0].1 - head).length() < 1e-4, "at the head");

        let stars = h.stars();
        assert_eq!(stars.len(), 12, "three points, four stars each");
        let mut points = std::collections::BTreeSet::new();
        for (star, g, material) in &stars {
            points.insert((star.point, star.trail));
            let k = star.point as usize;
            let local = g.translation() - head;
            let r = local.xz().length();
            assert!((r - STAR_ORBIT_RADII[k]).abs() < 1e-3, "orbit {r}");
            assert!(g.translation().y > head.y, "above the head");
            // Camera-facing, rolled by its place on the orbit.
            assert!(faces_camera(g, cam), "star {star:?} at bearing {bearing}");
            let angle = local.z.atan2(local.x);
            let up = g.rotation() * Vec3::Y;
            let rolled = cam * Quat::from_rotation_z(-angle) * Vec3::Y;
            assert!(up.dot(rolled) > 0.999, "rolled along the orbit");
            // Drawn as the bench draws it: an additive, unlit star, held at
            // its alpha once faded in.
            assert_eq!(material.alpha_mode, AlphaMode::Add);
            assert!(material.unlit);
            let want = 0.6 - star.trail as f32 * 0.13;
            assert!(
                (material.base_color.alpha() - want).abs() < 1e-4,
                "{star:?} alpha {}",
                material.base_color.alpha()
            );
        }
        assert_eq!(points.len(), 12, "every star once");
        let leads: Vec<[u8; 3]> = stars
            .iter()
            .filter(|(s, _, _)| s.trail == 0)
            .map(|(_, _, m)| srgb_u8(m.base_color))
            .collect();
        assert_eq!(leads, vec![[180, 0, 247]; 3], "the leads are violet");
        let tails: Vec<[u8; 3]> = stars
            .iter()
            .filter(|(s, _, _)| s.trail == 3)
            .map(|(_, _, m)| srgb_u8(m.base_color))
            .collect();
        assert!(
            tails.iter().all(|c| c[1] > 200),
            "the tails whiten: {tails:?}"
        );

        // Sparkles: falling straight down through the box above the head.
        let sparkles = h.sparkles();
        assert!(sparkles.len() > 50, "{} sparkles", sparkles.len());
        for (g, v, _) in &sparkles {
            assert_eq!(*v, Vec3::new(0.0, -3.33, 0.0));
            let p = g.translation() - head;
            assert!(p.x.abs() <= 0.77 && p.z.abs() <= 0.77, "{p} in the box");
            assert!(p.y <= 0.7 + 1e-3 && p.y >= 0.5 - 3.33 * 0.5 - 0.05, "{p}");
            assert!(faces_camera(g, cam), "sparkle faces the camera");
        }
    }
}

/// The two emitters keep 222 sparkles a second for half a second each: the
/// swirl holds about 111, and drops each as it is spent rather than keeping
/// it for the whole incapacitate.
#[test]
fn the_swirl_drops_its_spent_sparkles() {
    let mut h = Harness::new();
    let (paladin, mage, _) = paladin_and_mage(&mut h);
    h.repent(paladin, mage);
    for secs in [1.0, 2.0, 4.0] {
        h.tick_secs(secs - h.app.world().resource::<Time>().elapsed_secs() + DT);
        let held = h.sparkle_entities();
        assert!(
            (95..135).contains(&held),
            "{held} sparkles held at {secs} s"
        );
    }
}

/// The stars circle 0.6 turns a second.
#[test]
fn the_stars_circle_the_head() {
    let mut h = Harness::new();
    let (paladin, mage, at) = paladin_and_mage(&mut h);
    h.repent(paladin, mage);
    h.tick_secs(0.5);
    let head = at + Vec3::Y * HEAD_ABOVE_CENTRE;
    let lead = |h: &mut Harness| {
        let (_, g, _) = h
            .stars()
            .into_iter()
            .find(|(s, _, _)| s.point == 0 && s.trail == 0)
            .unwrap();
        let p = g.translation() - head;
        p.z.atan2(p.x)
    };
    let a0 = lead(&mut h);
    h.tick(25); // 0.4 s
    let turned = (lead(&mut h) - a0).rem_euclid(TAU);
    assert!(
        (turned - 0.4 * 0.6 * TAU).abs() < 0.02,
        "{turned} rad in 0.4 s"
    );
}

/// Every sparkle is the bench's: an additive star sprite, its palette the
/// two emitters' ramps (sky blue or indigo, through violet or lavender, to
/// white) and its alpha in from nothing to full at half-life and out.
#[test]
fn the_sparkles_draw_the_benchs_two_ramps() {
    let mut h = Harness::new();
    let (paladin, mage, _) = paladin_and_mage(&mut h);
    h.repent(paladin, mage);
    h.tick_secs(0.3);
    let palettes: Vec<Vec<Handle<StandardMaterial>>> = {
        let mut q = h.app.world_mut().query::<&ClientParticle>();
        let mut seen: Vec<Vec<Handle<StandardMaterial>>> = Vec::new();
        for p in q.iter(h.app.world()).filter(|p| p.owned) {
            let palette: Vec<_> = p.palette.iter().cloned().collect();
            if !seen.contains(&palette) {
                seen.push(palette);
            }
        }
        seen
    };
    assert_eq!(palettes.len(), 2, "two emitters");
    let star = star_flash_texture();
    let world = h.app.world();
    let (materials, images) = (
        world.resource::<Assets<StandardMaterial>>(),
        world.resource::<Assets<Image>>(),
    );
    let ramps = [
        [
            [104.0, 182.0, 242.0],
            [184.0, 0.0, 245.0],
            [255.0, 255.0, 255.0],
        ],
        [
            [102.0, 35.0, 225.0],
            [162.0, 77.0, 241.0],
            [255.0, 255.0, 255.0],
        ],
    ];
    let mut matched = [false; 2];
    for palette in &palettes {
        let n = palette.len() as f32;
        let at = |i: usize| materials.get(&palette[i]).unwrap();
        for (r, ramp) in ramps.iter().enumerate() {
            let fits = palette.iter().enumerate().all(|(i, _)| {
                let t = (i as f32 + 0.5) / n;
                let (a, b, u) = if t < 0.5 {
                    (ramp[0], ramp[1], t / 0.5)
                } else {
                    (ramp[1], ramp[2], (t - 0.5) / 0.5)
                };
                let want: [f32; 3] = [0, 1, 2].map(|c| a[c] + (b[c] - a[c]) * u);
                let got = srgb_u8(at(i).base_color);
                (0..3).all(|c| (got[c] as f32 - want[c]).abs() <= 1.0)
            });
            if fits {
                matched[r] = true;
            }
        }
        for (i, handle) in palette.iter().enumerate() {
            let m = materials.get(handle).unwrap();
            assert_eq!(m.alpha_mode, AlphaMode::Add);
            let texture = m.base_color_texture.as_ref().expect("a sprite");
            assert_eq!(images.get(texture).unwrap().data, star.data, "a star");
            let t = (i as f32 + 0.5) / n;
            let want = if t < 0.5 { t / 0.5 } else { (1.0 - t) / 0.5 };
            assert!((m.base_color.alpha() - want).abs() < 1e-4, "alpha {t}");
        }
    }
    assert_eq!(matched, [true, true], "both ramps drawn");
    // The stars' sprite is the same star.
    let stars = h.stars();
    let images = h.app.world().resource::<Assets<Image>>();
    for (_, _, m) in stars {
        let texture = m.base_color_texture.as_ref().unwrap();
        assert_eq!(images.get(texture).unwrap().data, star.data);
    }
}

// ── how it ends ─────────────────────────────────────────────────────────────

/// Damage breaks Repentance (`process_aura_breaks`), and the swirl goes
/// with it — stars, sparkles and all — and is not raised again.
#[test]
fn the_swirl_ends_when_damage_breaks_repentance() {
    let mut h = Harness::new();
    let (paladin, mage, _) = paladin_and_mage(&mut h);
    h.repent(paladin, mage);
    h.tick_secs(1.0);
    assert_eq!(h.swirls().len(), 1);
    assert!(!h.sparkles().is_empty() && h.count::<RepentanceStar>() == 12);
    h.app
        .world_mut()
        .entity_mut(mage)
        .insert(DamageTakenThisFrame { amount: 12.0 });
    h.tick(2);
    assert!(h.auras(mage).is_empty(), "the damage broke it");
    assert!(h.swirl_gone(), "the swirl breaks with it");
    h.tick(10);
    assert!(h.swirl_gone(), "and stays gone");
}

/// A dispel takes Repentance (`process_dispels`), and the swirl with it.
#[test]
fn the_swirl_ends_when_repentance_is_dispelled() {
    let mut h = Harness::new();
    let (paladin, mage, _) = paladin_and_mage(&mut h);
    let priest = h.unit(2, CharacterClass::Priest, Vec3::new(6.0, 1.0, 4.0));
    h.repent(paladin, mage);
    h.tick_secs(1.0);
    assert_eq!(h.swirls().len(), 1);
    h.app.world_mut().spawn(DispelPending {
        target: mage,
        dispeller: priest,
        log_prefix: "[DISPEL]",
        caster_class: CharacterClass::Priest,
        heal_on_success: None,
        scope: DispelScope::Magic,
    });
    h.tick(2);
    assert!(!h.auras(mage).contains(&REPENTANCE_AURA.to_string()));
    assert!(h.swirl_gone(), "the dispel takes the swirl");
}

/// Repentance runs out (`update_auras`, which removes the victim's last
/// aura's component): the stars fade over its last 0.2 s and the swirl goes
/// when it does.
#[test]
fn the_swirl_fades_out_and_ends_when_repentance_expires() {
    let mut h = Harness::new();
    let (paladin, mage, _) = paladin_and_mage(&mut h);
    h.repent(paladin, mage);
    h.tick(1);
    let alpha = |h: &mut Harness| {
        h.stars()
            .iter()
            .find(|(s, _, _)| s.point == 1 && s.trail == 0)
            .map(|(_, _, m)| m.base_color.alpha())
            .unwrap()
    };
    let left = |h: &Harness| h.app.world().get::<ActiveAuras>(mage).unwrap().auras[0].duration;
    h.tick(2);
    let early = alpha(&mut h);
    assert!(early > 0.0 && early < 0.6 * 0.5, "fading in: {early}");
    // On to 0.1 s before it runs out.
    h.tick_secs(left(&h) - 0.1);
    let remaining = left(&h);
    assert!((0.05..0.15).contains(&remaining), "{remaining}");
    let late = alpha(&mut h);
    let want = 0.6 * remaining / 0.2;
    assert!(
        (late - want).abs() < 0.6 * DT / 0.2 + 1e-3,
        "{late} vs {want}"
    );
    h.tick_secs(0.2);
    assert!(
        h.app.world().get::<ActiveAuras>(mage).is_none(),
        "the last aura's component is removed"
    );
    assert!(h.swirl_gone(), "the swirl ends with it");
}

/// A killing blow leaves the aura on the corpse (`process_aura_breaks`
/// skips the dead); the swirl goes anyway, and nothing is raised over the
/// corpse while the aura ticks out on it.
#[test]
fn nothing_swirls_over_a_corpse() {
    let mut h = Harness::new();
    let (paladin, mage, _) = paladin_and_mage(&mut h);
    h.repent(paladin, mage);
    h.tick_secs(1.0);
    assert_eq!(h.swirls().len(), 1);
    h.app
        .world_mut()
        .get_mut::<Combatant>(mage)
        .unwrap()
        .current_health = 0.0;
    h.tick(2);
    assert_eq!(
        h.auras(mage),
        vec![REPENTANCE_AURA.to_string()],
        "the aura stays on the corpse"
    );
    assert!(h.swirl_gone(), "the swirl does not");
    h.tick_secs(1.0);
    assert!(h.swirl_gone(), "and is not raised over it");

    // A victim despawned outright takes its swirl too.
    let mut h = Harness::new();
    let (paladin, mage, _) = paladin_and_mage(&mut h);
    h.repent(paladin, mage);
    h.tick_secs(0.5);
    assert_eq!(h.swirls().len(), 1);
    h.app.world_mut().entity_mut(mage).despawn();
    h.tick(2);
    assert!(h.swirl_gone());
}

/// A swirl despawned out from under a held incapacitate (the sandbox's
/// leftover sweep) is raised again; only one ever rings a head.
#[test]
fn a_held_incapacitate_keeps_exactly_one_swirl() {
    let mut h = Harness::new();
    let (paladin, mage, _) = paladin_and_mage(&mut h);
    h.repent(paladin, mage);
    h.tick_secs(0.5);
    let rigs: Vec<Entity> = {
        let mut q = h
            .app
            .world_mut()
            .query_filtered::<Entity, With<RepentanceSwirl>>();
        q.iter(h.app.world()).collect()
    };
    assert_eq!(rigs.len(), 1);
    h.app.world_mut().entity_mut(rigs[0]).despawn();
    h.tick(2);
    assert_eq!(h.swirls().len(), 1, "raised again");
    h.tick(30);
    assert_eq!(h.swirls().len(), 1);
    assert_eq!(h.count::<RepentanceStar>(), 12);
}

/// The swirl follows its victim's head wherever the victim stands.
#[test]
fn the_swirl_follows_the_victims_head() {
    let mut h = Harness::new();
    let (paladin, mage, _) = paladin_and_mage(&mut h);
    h.repent(paladin, mage);
    h.tick_secs(0.3);
    let moved = Vec3::new(-7.0, 1.0, 9.0);
    h.app
        .world_mut()
        .get_mut::<Transform>(mage)
        .unwrap()
        .translation = moved;
    h.tick(2);
    let swirls = h.swirls();
    assert_eq!(swirls.len(), 1);
    let head = moved + Vec3::Y * HEAD_ABOVE_CENTRE;
    assert!(
        (swirls[0].1 - head).length() < 1e-4,
        "{} vs {head}",
        swirls[0].1
    );
    for (star, g, _) in h.stars() {
        let r = (g.translation() - head).xz().length();
        assert!((r - STAR_ORBIT_RADII[star.point as usize]).abs() < 1e-3);
    }
}

// ── never the ice block ─────────────────────────────────────────────────────

/// Repentance raises no ice block, and an ice block whose victim holds only
/// Repentance's incapacitate is retired; Freezing Trap's keeps its block and
/// raises no swirl.
#[test]
fn only_freezing_trap_holds_an_ice_block() {
    let mut h = Harness::new();
    let (paladin, mage, _) = paladin_and_mage(&mut h);
    let hunter = h.unit(1, CharacterClass::Hunter, Vec3::new(-6.0, 1.0, 3.0));
    let rogue = h.unit(2, CharacterClass::Rogue, Vec3::new(0.0, 1.0, -6.0));
    h.repent(paladin, mage);
    h.app.world_mut().spawn(AuraPending {
        target: rogue,
        aura: freezing_trap_aura(hunter),
    });
    h.tick(2);
    assert_eq!(h.count::<IceBlockVisual>(), 0, "Repentance raises no block");
    assert_eq!(h.auras(rogue), vec![FREEZING_TRAP_AURA.to_string()]);
    assert_eq!(
        h.swirls().iter().map(|(o, _)| *o).collect::<Vec<_>>(),
        vec![mage],
        "the trapped Rogue gets no swirl"
    );
    // A block over each (as a stale trap trigger would leave one).
    for target in [mage, rogue] {
        h.app.world_mut().spawn(IceBlockVisual {
            target,
            grace_timer: 0.0,
        });
    }
    h.tick(2);
    let blocks: Vec<Entity> = {
        let mut q = h.app.world_mut().query::<&IceBlockVisual>();
        q.iter(h.app.world()).map(|b| b.target).collect()
    };
    assert_eq!(
        blocks,
        vec![rogue],
        "only Freezing Trap's victim keeps its block"
    );
}

// ── at the match camera ─────────────────────────────────────────────────────

/// Through the default match camera: every star stands above the head's
/// top, so the victim's own body never hides one, and the orbit reads wider
/// than the head on a 1280x720 screen at every yaw.
#[test]
fn the_swirl_reads_at_the_match_camera() {
    let zoom = CameraController::default().zoom_distance;
    let viewport = Vec2::new(1280.0, 720.0);
    let mut h = Harness::new();
    let (paladin, mage, at) = paladin_and_mage(&mut h);
    h.repent(paladin, mage);
    h.tick_secs(0.6);
    let stars = h.stars();
    let head_top = at.y + HEAD_ABOVE_CENTRE;
    for (star, g, _) in &stars {
        assert!(
            g.translation().y > head_top + 0.1,
            "{star:?} above the head"
        );
    }
    let head = at + Vec3::Y * HEAD_ABOVE_CENTRE;
    for step in 0..8 {
        let cam = match_camera(at, zoom, step as f32 * TAU / 8.0);
        let xs: Vec<f32> = stars
            .iter()
            .map(|(_, g, _)| project(&cam, g.translation(), viewport).x)
            .collect();
        let span = xs.iter().cloned().fold(f32::MIN, f32::max)
            - xs.iter().cloned().fold(f32::MAX, f32::min);
        let head_width = (project(&cam, head + cam.right() * 0.5, viewport)
            - project(&cam, head - cam.right() * 0.5, viewport))
        .length();
        assert!(
            span > 1.5 * head_width,
            "yaw {step}: orbit {span}px, head {head_width}px"
        );
    }
}
