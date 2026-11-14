//! Probes for the Paladin talent one-shots
//! (`rendering/effects/paladin_oneshots.rs`, AS-374): Divine Favor, Aura
//! Mastery, Blessing of Sacrifice, Retribution Aura, Infusion of Light and
//! The Art of War, and the instant hand flash they share.
//!
//! These drive the graphical chain off the sim's `AuraLanding` markers, as
//! `apply_pending_auras` spawns them, and assert WORLD geometry through
//! `GlobalTransform`: where each rig plays (the Paladin only, both ends, the
//! feet, the chest, above the head), which pieces face the camera and which
//! lie flat, how far the Retribution arrows point and which way, how the Art
//! of War ring opens, and when each one-shot retires. The tables' numbers are
//! pinned against the bench in the module's own tests.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window,
//! no GPU. The systems run in the order `states/mod.rs` registers them.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::AbilityType;
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    AuraApplyOwner, AuraApplyRoute, AuraLanding, AuraLandingKind, AuraType, CastEnding,
    CastEndingKind, ClientParticle, Combatant, HealCastHand, HealCastKind, HealCastPhase,
    HealCastPiece, HealCastPieceRole, LandedCast, OneShotPieceRole, PaladinOneShot,
    PaladinOneShotKind, PaladinOneShotPiece, ParticleDrag, VisualBody, ART_OF_WAR_AURA,
    AURA_MASTERY_AURA, BLESSING_OF_SACRIFICE_AURA, DIVINE_FAVOR_AURA, INFUSION_OF_LIGHT_AURA,
    RETRIBUTION_AURA_NAME,
};
use arenasim::states::play_match::{
    animate_client_particles, billboard_hunter_shots, consume_cast_ending_signals,
    holy_instant_flash_alpha, oneshot_secs, spawn_instant_cast_hands, spawn_paladin_oneshots,
    update_heal_cast_glows, update_paladin_oneshots, AURA_MASTERY_SECS, HOLY_INSTANT_FLASH_SECS,
    RETRIBUTION_AURA_SECS,
};
use arenasim::states::play_match::{
    blob_texture, heal_cast_glow_color, heal_cast_wisp_color, oneshot_glow_texture,
    oneshot_glyph_texture, oneshot_ground_texture, oneshot_ring_texture, star_flash_texture,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
const DT: f32 = 0.016;

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
        app.add_systems(
            Update,
            (
                spawn_instant_cast_hands,
                consume_cast_ending_signals,
                spawn_paladin_oneshots,
                update_paladin_oneshots,
                update_heal_cast_glows,
                animate_client_particles,
                billboard_hunter_shots,
            )
                .chain(),
        );
        Harness { app }
    }

    /// A unit as `spawn_combatant` builds it: capsule centre a yard above the
    /// floor, with a `VisualBody` child for the hands to hang off.
    fn unit(&mut self, class: CharacterClass, at: Vec3) -> Entity {
        let world = self.app.world_mut();
        let unit = world
            .spawn((Combatant::new(1, 0, class), Transform::from_translation(at)))
            .id();
        let body = world
            .spawn((VisualBody { rest_y: 0.0 }, Transform::default()))
            .id();
        world.entity_mut(unit).add_child(body);
        unit
    }

    fn camera(&mut self, from: Vec3, at: Vec3) -> Quat {
        let transform = Transform::from_translation(from).looking_at(at, Vec3::Y);
        self.app.world_mut().spawn((Camera3d::default(), transform));
        transform.rotation
    }

    fn land(&mut self, target: Entity, kind: AuraLandingKind, caster: Option<Entity>) {
        self.app.world_mut().spawn(AuraLanding {
            target,
            kind,
            caster,
        });
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn tick_secs(&mut self, secs: f32) {
        self.tick((secs / DT).ceil() as u32);
    }

    fn rigs(&mut self) -> Vec<(PaladinOneShotKind, Entity, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&PaladinOneShot, &GlobalTransform)>();
        q.iter(self.app.world())
            .map(|(r, g)| (r.kind, r.bearer, g.translation()))
            .collect()
    }

    fn pieces(&mut self) -> Vec<(OneShotPieceRole, GlobalTransform, f32)> {
        let mut q = self.app.world_mut().query::<(
            &PaladinOneShotPiece,
            &GlobalTransform,
            &MeshMaterial3d<StandardMaterial>,
        )>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .map(|(p, g, m)| (p.role, *g, m.0.clone()))
            .collect();
        let materials = self.app.world().resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(role, g, m)| {
                let alpha = materials.get(&m).map_or(0.0, |m| m.base_color.alpha());
                (role, g, alpha)
            })
            .collect()
    }

    fn piece(&mut self, role: OneShotPieceRole) -> (GlobalTransform, f32) {
        let found: Vec<_> = self
            .pieces()
            .into_iter()
            .filter(|(r, _, _)| *r == role)
            .collect();
        assert_eq!(found.len(), 1, "one {role:?}");
        (found[0].1, found[0].2)
    }

    fn hands(&mut self) -> Vec<(HealCastKind, Entity)> {
        let mut q = self.app.world_mut().query::<&HealCastHand>();
        q.iter(self.app.world())
            .map(|h| (h.kind, h.caster))
            .collect()
    }

    /// The hand glows' base alpha and drawn alpha.
    fn hand_glows(&mut self) -> Vec<(f32, f32)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&HealCastPiece, &MeshMaterial3d<StandardMaterial>)>();
        let rows: Vec<_> = q
            .iter(self.app.world())
            .filter(|(p, _)| {
                matches!(
                    p.role,
                    HealCastPieceRole::GlowOuter | HealCastPieceRole::GlowCore
                )
            })
            .map(|(p, m)| (p.base_alpha, m.0.clone()))
            .collect();
        let materials = self.app.world().resource::<Assets<StandardMaterial>>();
        rows.into_iter()
            .map(|(base, m)| (base, materials.get(&m).unwrap().base_color.alpha()))
            .collect()
    }

    fn particles(&mut self) -> Vec<(Vec3, Vec3)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&ClientParticle, &Transform)>();
        q.iter(self.app.world())
            .map(|(p, t)| (t.translation, p.velocity))
            .collect()
    }

    fn markers(&mut self) -> usize {
        let mut q = self.app.world_mut().query::<&AuraLanding>();
        q.iter(self.app.world()).count()
    }
}

/// A camera-facing quad's normal (its local +Z) lies along the camera's.
fn faces_camera(g: &GlobalTransform, camera: Quat) -> bool {
    (g.rotation() * Vec3::Z).dot(camera * Vec3::Z) > 0.999
}

/// A flat quad's normal points straight up.
fn lies_flat(g: &GlobalTransform) -> bool {
    (g.rotation() * Vec3::Z).dot(Vec3::Y) > 0.999
}

// ── routing ─────────────────────────────────────────────────────────────────

/// Each aura the real config lands carries the name its landing routes by,
/// and the landing owns that aura's application, so the band does not draw
/// it twice. Its type siblings keep the band.
#[test]
fn each_talent_aura_routes_to_its_landing_and_owns_its_application() {
    let defs = AbilityDefinitions::default();
    let owned = AuraApplyRoute::Owned(AuraApplyOwner::AuraLanding);
    for (ability, name, landing) in [
        (
            AbilityType::DivineFavor,
            DIVINE_FAVOR_AURA,
            AuraLandingKind::DivineFavorBurst,
        ),
        (
            AbilityType::AuraMastery,
            AURA_MASTERY_AURA,
            AuraLandingKind::AuraMasteryWard,
        ),
        (
            AbilityType::BlessingOfSacrifice,
            BLESSING_OF_SACRIFICE_AURA,
            AuraLandingKind::SacrificeShell,
        ),
        (
            AbilityType::RetributionAura,
            RETRIBUTION_AURA_NAME,
            AuraLandingKind::RetributionDisc,
        ),
    ] {
        let def = defs.get(&ability).expect("in the config");
        assert_eq!(def.name, name, "{ability:?}'s aura name");
        let t = def.applies_aura.as_ref().expect("lands an aura").aura_type;
        assert_eq!(AuraLandingKind::for_aura(t, name), Some(landing));
        assert_eq!(AuraApplyRoute::for_instance(t, name), owned, "{ability:?}");
        assert_eq!(
            AuraApplyRoute::for_instance(t, "Some Other Aura"),
            AuraApplyRoute::for_aura(t),
            "{t:?}'s other auras route by type"
        );
    }
    // The two procs prime the next cast.
    for (name, landing) in [
        (INFUSION_OF_LIGHT_AURA, AuraLandingKind::InfusionHalo),
        (ART_OF_WAR_AURA, AuraLandingKind::ArtOfWarRing),
    ] {
        assert_eq!(
            AuraLandingKind::for_aura(AuraType::NextCast, name),
            Some(landing)
        );
        assert_eq!(
            AuraApplyRoute::for_instance(AuraType::NextCast, name),
            owned
        );
    }
    // A primed cast with no one-shot keeps the band. (Shadow Trance has its
    // own landing since AS-346.)
    assert_eq!(
        AuraApplyRoute::for_instance(AuraType::NextCast, "A Primed Cast With No Landing"),
        AuraApplyRoute::Band
    );
    // The hands: Divine Favor's are Moonfire's arcane kit; Aura Mastery's and
    // Sacrifice's the instant Holy flash. All three ride the landing.
    assert_eq!(
        HealCastKind::for_ability(AbilityType::DivineFavor),
        Some(HealCastKind::Arcane)
    );
    for ability in [AbilityType::AuraMastery, AbilityType::BlessingOfSacrifice] {
        assert_eq!(
            HealCastKind::for_ability(ability),
            Some(HealCastKind::HolyFlash)
        );
    }
    for ability in [
        AbilityType::DivineFavor,
        AbilityType::AuraMastery,
        AbilityType::BlessingOfSacrifice,
    ] {
        assert!(AuraLandingKind::flashes_hands_for(ability), "{ability:?}");
    }
    assert!(!AuraLandingKind::flashes_hands_for(
        AbilityType::RetributionAura
    ));
}

/// The landing is consumed; another renderer's is not.
#[test]
fn only_the_paladin_landings_are_consumed() {
    let mut h = Harness::new();
    let paladin = h.unit(CharacterClass::Paladin, Vec3::new(0.0, 1.0, 0.0));
    h.land(paladin, AuraLandingKind::RejuvenationSwirl, None);
    h.land(paladin, AuraLandingKind::ArtOfWarRing, Some(paladin));
    h.tick(1);
    assert_eq!(
        h.markers(),
        1,
        "the Druid's marker is left for its renderer"
    );
    assert_eq!(h.rigs().len(), 1);
}

// ── Divine Favor ────────────────────────────────────────────────────────────

/// Divine Favor: the arcane hands and the innerfocus burst at the chest,
/// over in 0.4 s.
#[test]
fn divine_favor_bursts_at_the_chest_after_the_arcane_hands() {
    let mut h = Harness::new();
    let at = Vec3::new(3.0, 1.0, -2.0);
    let paladin = h.unit(CharacterClass::Paladin, at);
    h.land(paladin, AuraLandingKind::DivineFavorBurst, Some(paladin));
    h.tick(4);
    assert_eq!(
        h.rigs()
            .iter()
            .map(|(k, b, _)| (*k, *b))
            .collect::<Vec<_>>(),
        vec![(PaladinOneShotKind::DivineFavor, paladin)]
    );
    assert_eq!(h.hands(), vec![(HealCastKind::Arcane, paladin); 2]);
    let particles = h.particles();
    assert!(particles.len() >= 10, "{} particles", particles.len());
    // Born at the chest (1.45 yd up), within the burst's reach.
    for (p, _) in &particles {
        assert!((p.xz() - at.xz()).length() < 0.8, "{p}");
        assert!((p.y - 1.45).abs() < 0.8, "{p}");
    }
    h.tick_secs(oneshot_secs(PaladinOneShotKind::DivineFavor) + 0.05);
    assert!(h.rigs().is_empty(), "the burst retires");
}

// ── Aura Mastery ────────────────────────────────────────────────────────────

/// Aura Mastery wards the Paladin only: a teammate's landing plays nothing,
/// the Paladin's plays the 4.4 yd camera-facing ward and the Holy flash.
#[test]
fn aura_mastery_wards_the_paladin_only() {
    for bearing in [0.0_f32, FRAC_PI_2, 2.4] {
        let mut h = Harness::new();
        let at = Vec3::new(0.0, 1.0, 0.0);
        let paladin = h.unit(CharacterClass::Paladin, at);
        let mage = h.unit(CharacterClass::Mage, Vec3::new(4.0, 1.0, 0.0));
        let eye = at + Vec3::new(14.0 * bearing.sin(), 8.0, 14.0 * bearing.cos());
        let cam = h.camera(eye, at);
        h.land(mage, AuraLandingKind::AuraMasteryWard, Some(paladin));
        h.tick(2);
        assert!(h.rigs().is_empty(), "nothing on the teammate");
        assert!(h.hands().is_empty());

        h.land(paladin, AuraLandingKind::AuraMasteryWard, Some(paladin));
        h.tick_secs(0.3);
        // The star burst stalls under its drag (3 × 3.35 a second).
        let dragged: Vec<f32> = {
            let mut q = h.app.world_mut().query::<&ParticleDrag>();
            q.iter(h.app.world()).map(|d| d.per_sec).collect()
        };
        assert!(!dragged.is_empty(), "the star burst is dragged");
        assert!(
            dragged.iter().all(|d| (d - 10.05).abs() < 1e-4),
            "{dragged:?}"
        );
        h.tick_secs(1.2);
        let rigs = h.rigs();
        assert_eq!(rigs.len(), 1);
        assert_eq!(rigs[0].1, paladin);
        let (ward, alpha) = h.piece(OneShotPieceRole::WardGlow);
        assert!(faces_camera(&ward, cam), "the ward faces the camera");
        assert!((ward.translation() - Vec3::new(0.0, 1.45, 0.0)).length() < 1e-4);
        assert!((ward.scale().x - 4.4).abs() < 1e-4 && (ward.scale().y - 4.4).abs() < 1e-4);
        assert!(
            (alpha - 0.6 * 0.55).abs() < 1e-3,
            "held at 0.6 × 0.55: {alpha}"
        );
        // The ring's stars are born on the 1.0 yd ring round the chest.
        let ring: Vec<f32> = h
            .particles()
            .iter()
            .map(|(p, _)| (p.xz() - at.xz()).length())
            .collect();
        assert!(ring.iter().any(|r| (0.85..1.2).contains(r)), "{ring:?}");

        h.tick_secs(AURA_MASTERY_SECS - 1.5 + 0.05);
        assert!(h.rigs().is_empty(), "the ward retires at 3 s");
    }
}

/// The Holy flash: two hand rigs, gold, gone after 0.7 s, on the bench's
/// envelope.
#[test]
fn the_instant_holy_flash_plays_0_7_seconds_on_both_hands() {
    let mut h = Harness::new();
    let paladin = h.unit(CharacterClass::Paladin, Vec3::new(0.0, 1.0, 0.0));
    h.land(paladin, AuraLandingKind::AuraMasteryWard, Some(paladin));
    h.tick(2);
    assert_eq!(h.hands(), vec![(HealCastKind::HolyFlash, paladin); 2]);
    // It is the Holy glow and wisps, on the bench's envelope: 0.8 at 420 ms
    // (the launch re-flare would have them at a third of their 2x).
    h.tick(24); // 26 frames: 0.416 s
    let glows: Vec<(f32, f32)> = h.hand_glows();
    assert_eq!(glows.len(), 4, "the outer and core glow on both hands");
    // The flash's own clock, off its rig.
    let remaining: Vec<f32> = {
        let mut q = h.app.world_mut().query::<&HealCastHand>();
        q.iter(h.app.world())
            .map(|r| match r.phase {
                HealCastPhase::Flare { remaining } => remaining,
                HealCastPhase::Loop => panic!("an instant flash has no loop"),
            })
            .collect()
    };
    let want = holy_instant_flash_alpha(HOLY_INSTANT_FLASH_SECS - remaining[0]);
    assert!((0.7..0.85).contains(&want), "past the peak: {want}");
    for (base, alpha) in glows {
        assert!(
            (alpha - base * want).abs() < 2e-3,
            "{alpha} vs {base} × {want}"
        );
    }
    h.tick_secs(remaining[0] + 0.02);
    assert!(h.hands().is_empty(), "the flash is over at 0.7 s");
    assert_eq!(holy_instant_flash_alpha(0.0), 0.0);
    assert!((holy_instant_flash_alpha(0.08) - 1.0).abs() < 1e-5);
    assert!((holy_instant_flash_alpha(0.42) - 0.8).abs() < 1e-5);
    assert_eq!(holy_instant_flash_alpha(0.7), 0.0);
}

/// The animation sandbox lands these casts through a zero-length cast, so a
/// `Landed` marker names them too; the hands still flash once, off the
/// landing.
#[test]
fn a_landed_marker_does_not_flash_the_hands_twice() {
    let mut h = Harness::new();
    let paladin = h.unit(CharacterClass::Paladin, Vec3::new(0.0, 1.0, 0.0));
    for ability in [
        AbilityType::DivineFavor,
        AbilityType::AuraMastery,
        AbilityType::BlessingOfSacrifice,
    ] {
        h.app.world_mut().spawn((
            CastEnding {
                caster: paladin,
                kind: CastEndingKind::Landed,
            },
            LandedCast { ability },
        ));
        h.tick(2);
        assert!(h.hands().is_empty(), "{ability:?} flashes off its landing");
    }
    h.land(paladin, AuraLandingKind::DivineFavorBurst, Some(paladin));
    h.tick(2);
    assert_eq!(h.hands().len(), 2, "once, off the landing");
}

// ── Blessing of Sacrifice ───────────────────────────────────────────────────

/// Blessing of Sacrifice: the red shell at BOTH units' feet, the hands on the
/// Paladin, the motes falling, and no link between them.
#[test]
fn blessing_of_sacrifice_shells_both_ends() {
    let mut h = Harness::new();
    let (p_at, a_at) = (Vec3::new(-3.0, 1.0, 0.0), Vec3::new(4.0, 1.0, 2.0));
    let paladin = h.unit(CharacterClass::Paladin, p_at);
    let ally = h.unit(CharacterClass::Warrior, a_at);
    let cam = h.camera(Vec3::new(0.0, 9.0, 16.0), Vec3::ZERO);
    h.land(ally, AuraLandingKind::SacrificeShell, Some(paladin));
    h.tick(2);
    // The hands flash on the Paladin alone, for the 0.7 s flash.
    assert_eq!(h.hands(), vec![(HealCastKind::HolyFlash, paladin); 2]);
    h.tick_secs(1.0);

    let mut rigs = h.rigs();
    rigs.sort_by_key(|(k, _, _)| *k == PaladinOneShotKind::SacrificePaladin);
    assert_eq!(rigs.len(), 2);
    assert_eq!(
        (rigs[0].0, rigs[0].1),
        (PaladinOneShotKind::SacrificeAlly, ally)
    );
    assert_eq!(
        (rigs[1].0, rigs[1].1),
        (PaladinOneShotKind::SacrificePaladin, paladin)
    );
    // Each rig stands at its unit's feet.
    assert!((rigs[0].2 - Vec3::new(a_at.x, 0.0, a_at.z)).length() < 1e-4);
    assert!((rigs[1].2 - Vec3::new(p_at.x, 0.0, p_at.z)).length() < 1e-4);

    let pieces = h.pieces();
    let of = |role| {
        pieces
            .iter()
            .filter(|(r, _, _)| *r == role)
            .map(|(_, g, a)| (*g, *a))
            .collect::<Vec<_>>()
    };
    for (glow, alpha) in of(OneShotPieceRole::ShellGlow) {
        assert!(faces_camera(&glow, cam));
        assert!((glow.translation().y - 1.4).abs() < 1e-4);
        assert!((glow.scale().truncate() - Vec2::new(2.7, 2.475)).length() < 1e-4);
        assert!((alpha - 0.45).abs() < 1e-3, "held: {alpha}");
    }
    for role in [OneShotPieceRole::ShellRing, OneShotPieceRole::ShellGlyph] {
        let found = of(role);
        assert_eq!(found.len(), 2, "{role:?} at both ends");
        for (g, _) in found {
            assert!(lies_flat(&g), "{role:?} lies on the floor");
            assert!(g.translation().y < 0.06);
        }
    }
    // The motes fall straight down; no particle travels between the units.
    let falling = h
        .particles()
        .iter()
        .filter(|(_, v)| v.y < -1.6 && v.xz().length() < 1e-3)
        .count();
    assert!(falling > 50, "{falling} falling motes");

    h.tick_secs(1.0);
    assert!(h.rigs().is_empty(), "both ends retire at 1.9 s");
}

// ── Retribution Aura ────────────────────────────────────────────────────────

/// Retribution Aura: nothing on the other bearers; at the Paladin's feet a
/// flat disc whose four arrows point out on the diagonals and grow.
#[test]
fn retribution_aura_lays_its_disc_at_the_paladins_feet_only() {
    let mut h = Harness::new();
    let at = Vec3::new(2.0, 1.0, -1.0);
    let paladin = h.unit(CharacterClass::Paladin, at);
    let priest = h.unit(CharacterClass::Priest, Vec3::new(-2.0, 1.0, 0.0));
    h.camera(Vec3::new(0.0, 10.0, 15.0), Vec3::ZERO);
    h.land(priest, AuraLandingKind::RetributionDisc, Some(paladin));
    h.tick(2);
    assert!(h.rigs().is_empty(), "nothing on another bearer");

    h.land(paladin, AuraLandingKind::RetributionDisc, Some(paladin));
    h.tick_secs(0.95);
    assert!(h.hands().is_empty(), "no cast hands");
    let centre = Vec3::new(at.x, 0.0, at.z);
    let pieces = h.pieces();
    assert_eq!(pieces.len(), 7, "underlay, glyph, ring and four arrows");
    let mut directions = Vec::new();
    for (role, g, alpha) in &pieces {
        assert!(g.translation().y < 0.05, "{role:?} on the floor");
        match role {
            OneShotPieceRole::RetributionArrow { .. } => {
                let base = g.transform_point(Vec3::ZERO);
                let tip = g.transform_point(Vec3::X);
                let out = (tip - centre).xz().length();
                assert!(
                    out > (base - centre).xz().length() + 0.3,
                    "{role:?} points out"
                );
                // Base on the 0.86 yd circle; past it by 0.4 × the arrow scale.
                assert!((out - (0.8645 + 0.4 * 1.62)).abs() < 1e-3, "{out}");
                assert!(*alpha > 0.7, "the arrows are up at 0.95 s: {alpha}");
                directions.push((tip - centre).xz().normalize());
            }
            _ => assert!(lies_flat(g), "{role:?} lies flat"),
        }
    }
    // The four arrows on the diagonals.
    for k in 0..4 {
        let want = Vec2::from_angle(k as f32 * TAU / 4.0 + TAU / 8.0);
        assert!(
            directions.iter().any(|d| d.dot(want) > 0.9999),
            "an arrow along {want}"
        );
    }
    let (under, _) = h.piece(OneShotPieceRole::RetributionUnderlay);
    assert!((under.scale().x - 2.88).abs() < 1e-4, "2.9 yd across");

    h.tick_secs(RETRIBUTION_AURA_SECS - 0.95 + 0.05);
    assert!(h.rigs().is_empty(), "the disc retires at 1.43 s");
}

// ── Infusion of Light ───────────────────────────────────────────────────────

/// Infusion of Light: a flat gold ring 2.5 yd up, a camera-facing glow there,
/// and the overhead glows born at its height.
#[test]
fn infusion_of_light_pops_a_halo_above_the_head() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, 1.0, 5.0);
    let paladin = h.unit(CharacterClass::Paladin, at);
    let cam = h.camera(Vec3::new(8.0, 6.0, 15.0), at);
    h.land(paladin, AuraLandingKind::InfusionHalo, Some(paladin));
    h.tick_secs(1.0);
    let (ring, alpha) = h.piece(OneShotPieceRole::HaloRing);
    assert!(lies_flat(&ring));
    assert!((ring.translation() - Vec3::new(at.x, 2.5, at.z)).length() < 1e-4);
    assert!((ring.scale().x - 0.48).abs() < 1e-4);
    assert!((alpha - 0.55 * 1.4).abs() < 1e-3);
    let (glow, _) = h.piece(OneShotPieceRole::HaloGlow);
    assert!(faces_camera(&glow, cam));
    let high = h
        .particles()
        .iter()
        .filter(|(p, v)| (p.y - 2.5).abs() < 0.01 && *v == Vec3::ZERO)
        .count();
    assert!(high >= 5, "{high} overhead glows at the halo");
    h.tick_secs(0.9);
    assert!(h.rigs().is_empty());
}

// ── The Art of War ──────────────────────────────────────────────────────────

/// The Art of War: a flat disc at the chest, and a ring of camera-facing
/// sparks that opens out.
#[test]
fn the_art_of_war_ring_opens_round_the_chest() {
    let mut h = Harness::new();
    let at = Vec3::new(-1.0, 1.0, 1.0);
    let paladin = h.unit(CharacterClass::Paladin, at);
    let cam = h.camera(Vec3::new(-6.0, 7.0, 14.0), at);
    h.land(paladin, AuraLandingKind::ArtOfWarRing, Some(paladin));
    let spread = |h: &mut Harness| {
        let radii: Vec<f32> = h
            .pieces()
            .iter()
            .filter(|(r, _, a)| matches!(r, OneShotPieceRole::ArtOfWarSpark { .. }) && *a > 0.01)
            .map(|(_, g, _)| {
                assert!(faces_camera(g, cam));
                (g.translation().xz() - at.xz()).length()
            })
            .collect();
        assert!(!radii.is_empty(), "sparks are lit");
        radii.iter().sum::<f32>() / radii.len() as f32
    };
    h.tick_secs(0.2);
    let (disc, alpha) = h.piece(OneShotPieceRole::ArtOfWarDisc);
    assert!(lies_flat(&disc));
    assert!((disc.translation().y - 1.45).abs() < 1e-4);
    assert!(alpha > 0.5);
    let early = spread(&mut h);
    h.tick_secs(0.8);
    let late = spread(&mut h);
    assert!(late > early + 0.1, "the ring opens: {early} -> {late}");
    h.tick_secs(1.05);
    assert!(h.rigs().is_empty(), "retired at 2 s");
}

// ── what each piece is drawn with ───────────────────────────────────────────

/// The sprite the bench draws a piece with, as the generator that makes it.
#[derive(Debug)]
enum Sprite {
    Glow,
    Ground,
    Ring,
    Glyph(u32),
    Star,
    /// A flat-colour triangle: no texture, three vertices.
    Arrow,
}

/// Every role's colour (as the bench draws it; `None` for a spark, which
/// runs a ramp) and sprite — written out, not read from the module.
fn bench_piece(role: OneShotPieceRole) -> (Option<[u8; 3]>, Sprite) {
    use OneShotPieceRole as R;
    match role {
        R::WardGlow => (Some([255, 232, 130]), Sprite::Glow),
        R::ShellGlow => (Some([220, 10, 0]), Sprite::Glow),
        R::ShellRing => (Some([255, 54, 0]), Sprite::Ring),
        R::ShellGlyph => (Some([243, 137, 0]), Sprite::Glyph(8)),
        R::RetributionGlyph => (Some([252, 93, 12]), Sprite::Glyph(16)),
        R::RetributionRing => (Some([255, 0, 0]), Sprite::Ring),
        R::RetributionArrow { .. } => (Some([255, 162, 0]), Sprite::Arrow),
        R::RetributionUnderlay => (Some([255, 199, 29]), Sprite::Ground),
        R::HaloRing => (Some([237, 192, 72]), Sprite::Ring),
        R::HaloGlow => (Some([255, 230, 150]), Sprite::Glow),
        R::ArtOfWarDisc => (Some([221, 181, 57]), Sprite::Glyph(10)),
        R::ArtOfWarGlow => (Some([255, 230, 140]), Sprite::Glow),
        R::ArtOfWarSpark { .. } => (None, Sprite::Star),
    }
}

fn srgb_u8(c: Color) -> [u8; 3] {
    let c = c.to_srgba();
    [c.red, c.green, c.blue].map(|v| (v * 255.0).round() as u8)
}

/// Every piece of every one-shot, as spawned and drawn: its material's
/// colour is the bench's, and its texture (or mesh) is the bench's sprite.
#[test]
fn every_piece_draws_the_benchs_colour_and_sprite() {
    let mut h = Harness::new();
    let mut seen = std::collections::BTreeSet::new();
    for (i, kind) in [
        AuraLandingKind::AuraMasteryWard,
        AuraLandingKind::SacrificeShell,
        AuraLandingKind::RetributionDisc,
        AuraLandingKind::InfusionHalo,
        AuraLandingKind::ArtOfWarRing,
    ]
    .into_iter()
    .enumerate()
    {
        let paladin = h.unit(CharacterClass::Paladin, Vec3::new(i as f32 * 6.0, 1.0, 0.0));
        h.land(paladin, kind, Some(paladin));
    }
    // Late enough for the Art of War sparks to be lit, early enough that
    // every one-shot is still up.
    h.tick_secs(0.5);
    let rows: Vec<(OneShotPieceRole, Handle<StandardMaterial>, Handle<Mesh>)> = {
        let mut q = h.app.world_mut().query::<(
            &PaladinOneShotPiece,
            &MeshMaterial3d<StandardMaterial>,
            &Mesh3d,
        )>();
        q.iter(h.app.world())
            .map(|(p, m, mesh)| (p.role, m.0.clone(), mesh.0.clone()))
            .collect()
    };
    let world = h.app.world();
    let (materials, images, meshes) = (
        world.resource::<Assets<StandardMaterial>>(),
        world.resource::<Assets<Image>>(),
        world.resource::<Assets<Mesh>>(),
    );
    for (role, material, mesh) in rows {
        let (rgb, sprite) = bench_piece(role);
        let material = materials.get(&material).expect("its material");
        if let Some(rgb) = rgb {
            assert_eq!(srgb_u8(material.base_color), rgb, "{role:?} colour");
        }
        let expected = match sprite {
            Sprite::Glow => oneshot_glow_texture(),
            Sprite::Ground => oneshot_ground_texture(),
            Sprite::Ring => oneshot_ring_texture(),
            Sprite::Glyph(ticks) => oneshot_glyph_texture(ticks),
            Sprite::Star => star_flash_texture(),
            Sprite::Arrow => {
                assert!(material.base_color_texture.is_none(), "{role:?} is flat");
                assert_eq!(meshes.get(&mesh).unwrap().count_vertices(), 3);
                seen.insert(format!("{sprite:?}"));
                continue;
            }
        };
        let texture = material
            .base_color_texture
            .as_ref()
            .unwrap_or_else(|| panic!("{role:?} has a sprite"));
        assert_eq!(
            images.get(texture).unwrap().data,
            expected.data,
            "{role:?} is drawn with the bench's {sprite:?}"
        );
        seen.insert(format!("{sprite:?}"));
    }
    // Every sprite the bench uses is drawn somewhere: the probe is not vacuous.
    assert_eq!(seen.len(), 8, "{seen:?}");
}

/// Blessing of Sacrifice's motes are the bench's `blob`: a hard disc, not a
/// soft glow.
#[test]
fn the_sacrifice_motes_are_hard_blobs() {
    let mut h = Harness::new();
    let paladin = h.unit(CharacterClass::Paladin, Vec3::new(0.0, 1.0, 0.0));
    let ally = h.unit(CharacterClass::Warrior, Vec3::new(5.0, 1.0, 0.0));
    h.land(ally, AuraLandingKind::SacrificeShell, Some(paladin));
    h.tick_secs(0.3);
    let textures: Vec<Handle<Image>> = {
        let mut q = h
            .app
            .world_mut()
            .query::<(&ClientParticle, &MeshMaterial3d<StandardMaterial>)>();
        let rows: Vec<_> = q
            .iter(h.app.world())
            .filter(|(p, _)| (p.velocity - Vec3::new(0.0, -1.67, 0.0)).length() < 1e-4)
            .map(|(_, m)| m.0.clone())
            .collect();
        let materials = h.app.world().resource::<Assets<StandardMaterial>>();
        rows.iter()
            .map(|m| {
                materials
                    .get(m)
                    .unwrap()
                    .base_color_texture
                    .clone()
                    .unwrap()
            })
            .collect()
    };
    assert!(textures.len() > 10, "{} motes", textures.len());
    let images = h.app.world().resource::<Assets<Image>>();
    let blob = blob_texture();
    for texture in textures {
        assert_eq!(images.get(&texture).unwrap().data, blob.data, "a blob");
    }
}

/// The instant Holy flash IS the Holy hands: its glows and wisps take the
/// Holy family's colours, and its wisps the glow's soft sprite (Nature's are
/// star threads). The Sacred Shield, Divine Protection and Repentance cards
/// reuse it.
#[test]
fn the_instant_holy_flash_wears_the_holy_colours() {
    let mut h = Harness::new();
    let paladin = h.unit(CharacterClass::Paladin, Vec3::new(0.0, 1.0, 0.0));
    h.land(paladin, AuraLandingKind::AuraMasteryWard, Some(paladin));
    h.tick(3);
    let rows: Vec<(HealCastPieceRole, Handle<StandardMaterial>)> = {
        let mut q = h
            .app
            .world_mut()
            .query::<(&HealCastPiece, &MeshMaterial3d<StandardMaterial>)>();
        q.iter(h.app.world())
            .map(|(p, m)| (p.role, m.0.clone()))
            .collect()
    };
    let materials = h.app.world().resource::<Assets<StandardMaterial>>();
    let holy_glow = srgb_u8(heal_cast_glow_color(HealCastKind::Holy));
    assert_ne!(
        holy_glow,
        srgb_u8(heal_cast_glow_color(HealCastKind::Nature)),
        "Holy is not Nature"
    );
    let (mut glows, mut wisps) = (Vec::new(), Vec::new());
    for (role, material) in rows {
        let m = materials.get(&material).unwrap();
        match role {
            HealCastPieceRole::GlowOuter | HealCastPieceRole::GlowCore => {
                assert_eq!(srgb_u8(m.base_color), holy_glow, "{role:?}");
                glows.push(m.base_color_texture.clone().unwrap());
            }
            HealCastPieceRole::Wisp { index } => {
                assert_eq!(
                    srgb_u8(m.base_color),
                    srgb_u8(heal_cast_wisp_color(HealCastKind::Holy, index)),
                    "wisp {index}"
                );
                wisps.push(m.base_color_texture.clone().unwrap());
            }
            other => panic!("the Holy flash draws no {other:?}"),
        }
    }
    assert_eq!(
        (glows.len(), wisps.len()),
        (4, 6),
        "both hands' glows and wisps"
    );
    assert!(
        wisps.iter().all(|w| *w == glows[0]),
        "Holy wisps are ribbon-blur glows"
    );
}
