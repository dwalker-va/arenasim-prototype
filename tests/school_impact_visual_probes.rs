//! Probes for the shared, school-coloured impact
//! (`rendering/effects/school_impact.rs`) — the recolour tier under the
//! bespoke bolt bursts.
//!
//! These assert WORLD-SPACE GEOMETRY and the routing contract, not the fields
//! the rig stores: that the burst sits on the body and follows it, that the
//! head anchor is above the chest anchor, that splinters fall and splash back
//! toward the caster, that a smoulder rises, that a crit is visibly larger,
//! and that every projectile in the config reaches SOME landing.
//!
//! Runs on `MinimalPlugins` + `AssetPlugin` + `TransformPlugin` — no window, no
//! GPU. `TransformPlugin` is load-bearing: without it `GlobalTransform` never
//! propagates and every assertion below would read a child's LOCAL pose.

use std::time::Duration;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::play_match::abilities::{AbilityType, SpellSchool};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::components::{
    Combatant, ImpactAnchor, ImpactMote, ImpactRole, ImpactSprite, Pet, PetType, SchoolImpact,
};
use arenasim::states::play_match::{
    animate_school_impacts, bolt_kind_for, impact_origin, impact_rotation, impact_size,
    impact_style, landing_style, spawn_school_impacts, spray_count, spray_direction, SprayKind,
    IMPACT_CRIT_SCALE, IMPACT_MAGNITUDE_FLOOR, IMPACT_MAGNITUDE_FULL,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);

/// Bearings chosen so no single world axis can satisfy them all.
fn froms() -> Vec<Vec3> {
    vec![
        Vec3::X,
        -Vec3::X,
        Vec3::Z,
        Vec3::new(1.0, 0.0, 1.0).normalize(),
        Vec3::new(-0.3, 0.4, 0.86).normalize(),
    ]
}

fn all_schools() -> [SpellSchool; 8] {
    [
        SpellSchool::Physical,
        SpellSchool::Frost,
        SpellSchool::Holy,
        SpellSchool::Shadow,
        SpellSchool::Arcane,
        SpellSchool::Fire,
        SpellSchool::Nature,
        SpellSchool::None,
    ]
}

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
        app.add_systems(
            Update,
            (spawn_school_impacts, animate_school_impacts).chain(),
        );
        Harness { app }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }

    /// A victim the burst can attach to. It needs a real `Combatant`, because
    /// the burst follows one through a `With<Combatant>` query.
    fn spawn_victim(&mut self, at: Vec3) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, CharacterClass::Warrior),
                Transform::from_translation(at),
            ))
            .id()
    }

    fn spawn_pet_victim(&mut self, at: Vec3) -> Entity {
        let owner = self.spawn_victim(at + Vec3::X * 5.0);
        self.app
            .world_mut()
            .spawn((
                Combatant::new(0, 0, CharacterClass::Hunter),
                Pet {
                    owner,
                    pet_type: PetType::Spider,
                },
                Transform::from_translation(at),
            ))
            .id()
    }

    /// Land a hit on `victim`, as the spawn sites would, with a representative
    /// ability of the school (none of these override their school's row).
    fn land(
        &mut self,
        school: SpellSchool,
        anchor: ImpactAnchor,
        victim: Entity,
        from: Vec3,
        magnitude: f32,
        is_crit: bool,
    ) -> Entity {
        let ability = match school {
            SpellSchool::Physical => AbilityType::AimedShot,
            SpellSchool::Nature => AbilityType::SerpentSting,
            SpellSchool::Shadow => AbilityType::MindBlast,
            SpellSchool::Holy => AbilityType::HolyShock,
            _ => AbilityType::ArcaneShot,
        };
        self.land_ability(ability, school, anchor, victim, from, magnitude, is_crit)
    }

    #[allow(clippy::too_many_arguments)]
    fn land_ability(
        &mut self,
        ability: AbilityType,
        school: SpellSchool,
        anchor: ImpactAnchor,
        victim: Entity,
        from: Vec3,
        magnitude: f32,
        is_crit: bool,
    ) -> Entity {
        self.app
            .world_mut()
            .spawn(SchoolImpact {
                target: victim,
                ability,
                school,
                anchor,
                from: from.normalize_or_zero(),
                magnitude,
                is_crit,
                age: 0.0,
            })
            .id()
    }

    fn rig_pos(&mut self, rig: Entity) -> Vec3 {
        self.app
            .world()
            .get::<GlobalTransform>(rig)
            .expect("rig has a world transform")
            .translation()
    }

    /// Every entity carrying `T`, with its propagated world transform.
    fn global<T: Component>(&mut self) -> Vec<(Entity, GlobalTransform)> {
        let mut q = self
            .app
            .world_mut()
            .query_filtered::<(Entity, &GlobalTransform), With<T>>();
        q.iter(self.app.world()).map(|(e, g)| (e, *g)).collect()
    }

    fn sprites(&mut self, role: ImpactRole) -> Vec<GlobalTransform> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&ImpactSprite, &GlobalTransform)>();
        q.iter(self.app.world())
            .filter(|(s, _)| s.role == role)
            .map(|(_, g)| *g)
            .collect()
    }

    fn motes(&mut self, kind: SprayKind) -> Vec<(ImpactMoteView, GlobalTransform)> {
        let mut q = self
            .app
            .world_mut()
            .query::<(&ImpactMote, &GlobalTransform)>();
        q.iter(self.app.world())
            .filter(|(m, _)| m.kind == kind)
            .map(|(m, g)| {
                (
                    ImpactMoteView {
                        velocity: m.velocity,
                        age: m.age,
                    },
                    *g,
                )
            })
            .collect()
    }
}

struct ImpactMoteView {
    velocity: Vec3,
    age: f32,
}

fn centroid(points: &[Vec3]) -> Vec3 {
    points.iter().copied().sum::<Vec3>() / points.len().max(1) as f32
}

// ── routing ────────────────────────────────────────────────────────────────

/// Every projectile in the config must reach SOME landing.
///
/// This is the audit that keeps a future projectile from landing in silence:
/// it either has a bespoke burst (the two bolts, Death Coil), is Web — whose
/// source has no impact kit, only the root state `hard_cc.rs` draws — or it is
/// routed to the shared impact. A new `projectile_speed` in `abilities.ron`
/// with none of those fails here.
#[test]
fn every_projectile_in_the_config_reaches_a_landing() {
    let defs = AbilityDefinitions::default();
    let mut silent = Vec::new();
    for (ability, config) in defs.iter() {
        if config.projectile_speed.is_none() {
            continue;
        }
        let bespoke = bolt_kind_for(*ability).is_some() || *ability == AbilityType::DeathCoil;
        let state_only = *ability == AbilityType::SpiderWeb;
        let shared = SchoolImpact::anchor_for(*ability).is_some();
        if !(bespoke || state_only || shared) {
            silent.push(*ability);
        }
    }
    assert!(
        silent.is_empty(),
        "projectiles with no landing at all: {silent:?} — add an arm to \
         SchoolImpact::anchor_for or give them a bespoke burst"
    );
}

/// The router names exactly the intended landings, at the intended anchors.
#[test]
fn the_router_names_the_intended_landings() {
    for ability in [
        AbilityType::AimedShot,
        AbilityType::ArcaneShot,
        AbilityType::ConcussiveShot,
        AbilityType::SerpentSting,
        AbilityType::HolyShock,
        AbilityType::ManaBurn,
    ] {
        assert_eq!(
            SchoolImpact::anchor_for(ability),
            Some(ImpactAnchor::Chest),
            "{ability:?} lands on the chest (attachment 34 in the source)"
        );
    }
    assert_eq!(
        SchoolImpact::anchor_for(AbilityType::MindBlast),
        Some(ImpactAnchor::Head),
        "Mind Blast's source model is mindblast_head.m2 on attachment 20"
    );
    // Bespoke landings and non-landings must not double up.
    for ability in [
        AbilityType::Frostbolt,
        AbilityType::Shadowbolt,
        AbilityType::DeathCoil,
        AbilityType::LightningBolt,
        AbilityType::SpiderWeb,
        AbilityType::FlashHeal,
        AbilityType::Polymorph,
        AbilityType::MortalStrike,
    ] {
        assert_eq!(
            SchoolImpact::anchor_for(ability),
            None,
            "{ability:?} must not route to the shared impact"
        );
    }
    // A shared landing must not ALSO be a bespoke one.
    for ability in [AbilityType::AimedShot, AbilityType::ArcaneShot, AbilityType::MindBlast] {
        assert!(bolt_kind_for(ability).is_none());
    }
}

// ── the palette ────────────────────────────────────────────────────────────

/// Every school has a row, and every row ends.
#[test]
fn every_school_has_a_row_that_ends() {
    for school in all_schools() {
        let life = impact_style(school).life();
        assert!(
            life > 0.05 && life < 2.5,
            "{school:?}'s landing lasts {life}s — must be brief enough to clear \
             before the next cast"
        );
    }
}

/// Magic schools take their colour from `SpellSchool::color`, the authority;
/// Physical is the documented exception and must be hueless.
#[test]
fn colour_comes_from_the_school_authority() {
    for school in all_schools() {
        let style = impact_style(school);
        let c = style.color.to_srgba();
        if school == SpellSchool::Physical {
            let spread = c.red.max(c.green).max(c.blue) - c.red.min(c.green).min(c.blue);
            assert!(
                spread < 0.1,
                "Physical must be hueless (its tan is the floor colour), got {c:?}"
            );
            assert!(c.red > 0.85, "Physical must be bright enough to flash: {c:?}");
        } else {
            let a = school.color().to_srgba();
            assert!(
                (c.red - a.red).abs() < 1e-5
                    && (c.green - a.green).abs() < 1e-5
                    && (c.blue - a.blue).abs() < 1e-5,
                "{school:?}'s impact colour {c:?} drifted from the authority {a:?}"
            );
        }
    }
}

/// Additive can only brighten. On pale sand a landing needs something that
/// DARKENS or it reads as blended and slight — the Shadow Bolt lesson. The
/// three schools that actually land through this tier today each carry one.
#[test]
fn each_live_school_has_a_piece_that_can_darken() {
    let physical = impact_style(SpellSchool::Physical);
    let nature = impact_style(SpellSchool::Nature);
    let shadow = impact_style(SpellSchool::Shadow);
    assert!(
        matches!(physical.spray, Some(s) if !s.additive),
        "Physical splinters must be lit geometry, not additive glow"
    );
    assert!(
        matches!(nature.spray, Some(s) if !s.additive),
        "Nature's droplets are alpha-blended in the source and must stay so"
    );
    assert!(shadow.blot.is_some(), "Shadow needs its dark blot");
}

/// A ring is magic language. Physical and Nature have none; Arcane does.
#[test]
fn the_ring_is_only_magic_language() {
    assert!(impact_style(SpellSchool::Physical).ring.is_none());
    assert!(impact_style(SpellSchool::Nature).ring.is_none());
    assert!(impact_style(SpellSchool::Arcane).ring.is_some());

    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.land(SpellSchool::Physical, ImpactAnchor::Chest, victim, Vec3::X, 0.1, false);
    h.tick(1);
    assert!(h.sprites(ImpactRole::Ring).is_empty(), "a Physical hit spawned a ring");
    let other = h.spawn_victim(Vec3::Z * 6.0);
    h.land(SpellSchool::Arcane, ImpactAnchor::Chest, other, Vec3::X, 0.1, false);
    h.tick(1);
    assert_eq!(h.sprites(ImpactRole::Ring).len(), 1, "an Arcane hit spawns one ring");
}

// ── size ───────────────────────────────────────────────────────────────────

/// The size curve: a floor for aura-only landings, full at the reference
/// fraction, a crit on top.
#[test]
fn a_crit_lands_bigger_and_nothing_lands_smaller_than_the_floor() {
    assert!((impact_size(0.0, false) - IMPACT_MAGNITUDE_FLOOR).abs() < 1e-6);
    assert!((impact_size(IMPACT_MAGNITUDE_FULL, false) - 1.0).abs() < 1e-6);
    assert!((impact_size(10.0, false) - 1.0).abs() < 1e-6, "clamped above full");
    assert!(impact_size(0.05, false) > impact_size(0.0, false));
    assert!(impact_size(0.15, false) > impact_size(0.05, false));
    let plain = impact_size(0.1, false);
    let crit = impact_size(0.1, true);
    assert!((crit / plain - IMPACT_CRIT_SCALE).abs() < 1e-5);
    // A crit throws more debris too.
    let spray = impact_style(SpellSchool::Physical).spray.expect("Physical sprays");
    assert!(spray_count(&spray, true) > spray_count(&spray, false));
}

/// The crit read has to exist in WORLD space, not just in a scalar: the
/// crit's flash quad is measurably larger than a plain hit's at the same age.
#[test]
fn a_crit_flash_is_larger_on_screen() {
    let mut h = Harness::new();
    let a = h.spawn_victim(Vec3::ZERO);
    let b = h.spawn_victim(Vec3::Z * 8.0);
    h.land(SpellSchool::Arcane, ImpactAnchor::Chest, a, Vec3::X, 0.05, false);
    h.land(SpellSchool::Arcane, ImpactAnchor::Chest, b, Vec3::X, 0.25, true);
    h.tick(2);
    let flashes = h.sprites(ImpactRole::Flash);
    assert_eq!(flashes.len(), 2);
    let mut widths: Vec<(f32, f32)> = flashes
        .iter()
        .map(|g| (g.translation().z, g.scale().x))
        .collect();
    widths.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap());
    let (plain, crit) = (widths[0].1, widths[1].1);
    assert!(
        crit > plain * 1.3,
        "a crit flash ({crit:.3}) should dwarf a light hit ({plain:.3})"
    );
}

// ── anchors ────────────────────────────────────────────────────────────────

/// The chest anchor is ON the body and the head anchor is above it, both
/// inside the capsule — which is centred on the transform, spanning ±1.25.
#[test]
fn the_anchors_sit_on_the_capsule() {
    let chest = impact_origin(ImpactAnchor::Chest, Vec3::ZERO, false);
    let head = impact_origin(ImpactAnchor::Head, Vec3::ZERO, false);
    assert!(
        chest.y > -0.75 && chest.y < 0.75,
        "chest anchor {} is off the cylinder (±0.75 about the transform)",
        chest.y
    );
    assert!(
        head.y > 0.75 && head.y <= 1.25,
        "head anchor {} is not in the top hemisphere (0.75..1.25)",
        head.y
    );
    assert!(head.y - chest.y > 0.4, "the two anchors must read as different places");

    // A pet's body hangs BELOW its transform, at about half the stature.
    let pet_chest = impact_origin(ImpactAnchor::Chest, Vec3::ZERO, true);
    let pet_head = impact_origin(ImpactAnchor::Head, Vec3::ZERO, true);
    assert!(pet_chest.y < chest.y && pet_head.y < head.y);
    assert!(
        pet_chest.y > -1.1 && pet_head.y < 0.25,
        "pet anchors {pet_chest:?} / {pet_head:?} are off the pet capsule (-1.1..0.2)"
    );
    assert!(pet_head.y > pet_chest.y);
}

/// The rig yaws toward the caster and keeps world up as its own up, for every
/// bearing including pitched ones — its debris falls along local Y.
#[test]
fn the_rig_faces_the_caster_and_keeps_world_up() {
    for from in froms() {
        let rot = impact_rotation(from);
        let up = rot * Vec3::Y;
        assert!(up.dot(Vec3::Y) > 0.999, "rig tilted off world up for {from:?}: {up:?}");
        let z = rot * Vec3::Z;
        let flat = Vec3::new(from.x, 0.0, from.z).normalize();
        assert!(
            z.dot(flat) > 0.999,
            "rig's +Z {z:?} does not look back toward the caster {flat:?}"
        );
    }
    // A vertical bearing has no yaw to take; identity is the sane answer.
    assert_eq!(impact_rotation(Vec3::Y), Quat::IDENTITY);
    assert_eq!(impact_rotation(Vec3::ZERO), Quat::IDENTITY);
}

/// The burst rides its victim, at the anchor, for both anchors and for a pet.
#[test]
fn the_landing_rides_a_moving_victim() {
    for anchor in [ImpactAnchor::Chest, ImpactAnchor::Head] {
        let mut h = Harness::new();
        let victim = h.spawn_victim(Vec3::ZERO);
        let rig = h.land(SpellSchool::Arcane, anchor, victim, Vec3::X, 0.1, false);
        h.tick(1);
        let first = h.rig_pos(rig);
        assert!((first - impact_origin(anchor, Vec3::ZERO, false)).length() < 1e-3);

        let moved = Vec3::new(2.5, 0.0, 1.5);
        if let Some(mut t) = h.world().get_mut::<Transform>(victim) {
            t.translation = moved;
        }
        h.tick(1);
        let after = h.rig_pos(rig);
        assert!(
            (after - impact_origin(anchor, moved, false)).length() < 1e-3,
            "{anchor:?} burst stayed at {after:?} while its victim moved to {moved:?}"
        );
    }

    let mut h = Harness::new();
    let pet = h.spawn_pet_victim(Vec3::ZERO);
    let rig = h.land(SpellSchool::Physical, ImpactAnchor::Chest, pet, Vec3::X, 0.1, false);
    h.tick(1);
    let at = h.rig_pos(rig);
    assert!(
        (at - impact_origin(ImpactAnchor::Chest, Vec3::ZERO, true)).length() < 1e-3,
        "a pet victim's burst must use the pet anchor, got {at:?}"
    );
}

// ── debris ─────────────────────────────────────────────────────────────────

/// Launch directions are unit length, folded UP by `lift` and BACK by `back`,
/// and unfolded they cover both hemispheres.
#[test]
fn spray_directions_fold_up_and_back() {
    let n = 16;
    for i in 0..n {
        for (lift, back) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (0.5, 0.5)] {
            let d = spray_direction(i, n, lift, back);
            assert!((d.length() - 1.0).abs() < 1e-4, "not unit: {d:?}");
            if lift >= 1.0 {
                assert!(d.y >= -1e-6, "lift=1 threw a piece downward: {d:?}");
            }
            if back >= 1.0 {
                assert!(d.z >= -1e-6, "back=1 threw a piece away from the caster: {d:?}");
            }
        }
    }
    let plain: Vec<Vec3> = (0..n).map(|i| spray_direction(i, n, 0.0, 0.0)).collect();
    assert!(plain.iter().any(|d| d.y > 0.3) && plain.iter().any(|d| d.y < -0.3));
    assert!(plain.iter().any(|d| d.z > 0.3) && plain.iter().any(|d| d.z < -0.3));
    let c = centroid(&plain);
    assert!(c.length() < 0.2, "an unfolded fan should be balanced, centroid {c:?}");
}

/// Physical splinters SPLASH BACK toward the caster and FALL — in world space,
/// for a bearing that is not a world axis.
#[test]
fn splinters_splash_back_toward_the_caster_and_fall() {
    let from = Vec3::new(1.0, 0.0, 1.0).normalize();
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.land(SpellSchool::Physical, ImpactAnchor::Chest, victim, from, 0.1, false);
    h.tick(4);
    let early: Vec<Vec3> = h
        .motes(SprayKind::Splinter)
        .iter()
        .map(|(_, g)| g.translation())
        .collect();
    assert!(!early.is_empty(), "no splinters were thrown");

    h.tick(12);
    let late: Vec<Vec3> = h
        .motes(SprayKind::Splinter)
        .iter()
        .map(|(_, g)| g.translation())
        .collect();
    assert_eq!(late.len(), early.len(), "splinters vanished before their life ended");

    let chest = impact_origin(ImpactAnchor::Chest, Vec3::ZERO, false);
    let drift = centroid(&late) - chest;
    let flat = Vec3::new(drift.x, 0.0, drift.z);
    assert!(
        flat.normalize_or_zero().dot(from) > 0.5,
        "splinters should splash back toward {from:?}, drifted {drift:?}"
    );
    assert!(
        centroid(&late).y < centroid(&early).y,
        "gravity should have pulled the splinters down between frames"
    );
    // The fan has real world extent — not a rosette clumped at one point.
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &late {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let extent = hi - lo;
    assert!(
        extent.x.max(extent.z) > 1.0 && extent.y > 0.4,
        "splinters span only {extent:?} after 0.26s"
    );
}

/// Mind Blast's landing smoulders: motes keep appearing after the hit and
/// they RISE from the head, in world space.
#[test]
fn the_head_smoulder_keeps_emitting_and_rises() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.land(SpellSchool::Shadow, ImpactAnchor::Head, victim, Vec3::X, 0.15, false);
    h.tick(6);
    let early = h.motes(SprayKind::Spark).len();
    h.tick(20);
    let sparks = h.motes(SprayKind::Spark);
    assert!(
        sparks.len() > early && early > 0,
        "a smoulder should keep emitting: {early} motes at 0.1s, {} at 0.4s",
        sparks.len()
    );
    let head = impact_origin(ImpactAnchor::Head, Vec3::ZERO, false);
    for (mote, g) in &sparks {
        assert!(mote.velocity.y > 0.0, "a smoulder mote must rise: {:?}", mote.velocity);
        assert!(
            g.translation().y >= head.y - 0.05,
            "a smoulder mote at {:?} fell below the head anchor {head:?}",
            g.translation()
        );
    }
    let oldest = sparks
        .iter()
        .max_by(|a, b| a.0.age.partial_cmp(&b.0.age).unwrap())
        .expect("some mote");
    assert!(
        oldest.1.translation().y > head.y + 0.2,
        "the oldest mote should have climbed clear of the head: {:?}",
        oldest.1.translation()
    );
    // The smoulder is a LINGERING landing, longer than any burst.
    assert!(impact_style(SpellSchool::Shadow).life() > impact_style(SpellSchool::Arcane).life());
}

/// Nature's droplets are many, small, and fall.
#[test]
fn droplets_fall() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.land(SpellSchool::Nature, ImpactAnchor::Chest, victim, Vec3::X, 0.0, false);
    h.tick(3);
    let early: Vec<Vec3> = h
        .motes(SprayKind::Drop)
        .iter()
        .map(|(_, g)| g.translation())
        .collect();
    assert!(early.len() >= 16, "a droplet burst should be dense, got {}", early.len());
    h.tick(12);
    let late: Vec<Vec3> = h
        .motes(SprayKind::Drop)
        .iter()
        .map(|(_, g)| g.translation())
        .collect();
    assert!(centroid(&late).y < centroid(&early).y - 0.1);
}

/// Mana Burn is Shadow without being Mind Blast: same colour authority, but a
/// CHEST landing whose sparks are pulled upward, brief, with no smoulder — the
/// client gives it its own model where Mind Blast smoulders on the head.
#[test]
fn mana_burn_overrides_the_shadow_row_without_leaving_its_colour() {
    let burn = landing_style(AbilityType::ManaBurn, SpellSchool::Shadow);
    let blast = landing_style(AbilityType::MindBlast, SpellSchool::Shadow);
    assert_eq!(blast, impact_style(SpellSchool::Shadow), "Mind Blast IS the Shadow row");
    assert_ne!(burn, blast, "Mana Burn must not read as Mind Blast");
    let (b, s) = (burn.color.to_srgba(), SpellSchool::Shadow.color().to_srgba());
    assert!((b.red - s.red).abs() < 1e-5 && (b.green - s.green).abs() < 1e-5 && (b.blue - s.blue).abs() < 1e-5);
    assert!(burn.smoulder.is_none() && burn.spray.is_some());
    assert!(burn.life() < blast.life() * 0.5, "a mana burn is a snap, not a smoulder");
    // Every other ability plays its school's row unchanged.
    for (ability, school) in [
        (AbilityType::AimedShot, SpellSchool::Physical),
        (AbilityType::ArcaneShot, SpellSchool::Arcane),
        (AbilityType::SerpentSting, SpellSchool::Nature),
        (AbilityType::HolyShock, SpellSchool::Holy),
    ] {
        assert_eq!(landing_style(ability, school), impact_style(school));
    }

    // And in world space: the sparks leave the CHEST going UP.
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.land_ability(
        AbilityType::ManaBurn,
        SpellSchool::Shadow,
        ImpactAnchor::Chest,
        victim,
        Vec3::X,
        0.18,
        false,
    );
    h.tick(3);
    let sparks = h.motes(SprayKind::Spark);
    assert!(sparks.len() >= 12, "expected a fan of sparks, got {}", sparks.len());
    let chest = impact_origin(ImpactAnchor::Chest, Vec3::ZERO, false);
    assert!(sparks.iter().all(|(m, _)| m.velocity.y > 0.0), "mana leaves upward");
    h.tick(20);
    let late = h.motes(SprayKind::Spark);
    let c = centroid(&late.iter().map(|(_, g)| g.translation()).collect::<Vec<_>>());
    assert!(c.y > chest.y + 0.3, "the fan should have risen clear of the chest: {c:?}");
}

// ── hygiene ────────────────────────────────────────────────────────────────

/// Every mesh a landing spawns is a `NotShadowCaster`, smoulder motes
/// included — the bolt trails painted a dotted black line on the floor before
/// anyone thought to check.
#[test]
fn no_part_of_a_landing_casts_a_shadow() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    for (i, school) in all_schools().iter().enumerate() {
        let anchor = if *school == SpellSchool::Shadow {
            ImpactAnchor::Head
        } else {
            ImpactAnchor::Chest
        };
        h.land(*school, anchor, victim, froms()[i % 5], 0.2, i % 2 == 0);
    }
    h.tick(8);
    let mut q = h
        .world()
        .query_filtered::<(Entity, Option<&NotShadowCaster>), With<Mesh3d>>();
    let mut meshes = 0;
    for (entity, flag) in q.iter(h.app.world()) {
        meshes += 1;
        assert!(flag.is_some(), "{entity:?} carries a mesh but casts shadows");
    }
    assert!(meshes > 30, "expected a lot of pieces across eight schools, saw {meshes}");
}

/// Every school's landing expires and takes all of its parts with it.
#[test]
fn landings_expire_with_all_their_parts() {
    for school in all_schools() {
        let mut h = Harness::new();
        let victim = h.spawn_victim(Vec3::ZERO);
        h.land(school, ImpactAnchor::Chest, victim, Vec3::X, 0.3, true);
        h.tick(2);
        assert_eq!(h.global::<SchoolImpact>().len(), 1, "{school:?} never spawned");
        let frames = (impact_style(school).life() / TICK.as_secs_f32()).ceil() as u32 + 3;
        h.tick(frames);
        assert!(h.global::<SchoolImpact>().is_empty(), "{school:?}'s landing never expired");
        assert!(
            h.global::<ImpactSprite>().is_empty() && h.global::<ImpactMote>().is_empty(),
            "{school:?}'s parts outlived their rig"
        );
    }
}

/// A landing on a victim that has since despawned must not panic and must
/// still expire.
#[test]
fn a_landing_survives_losing_its_victim() {
    let mut h = Harness::new();
    let victim = h.spawn_victim(Vec3::ZERO);
    h.land(SpellSchool::Arcane, ImpactAnchor::Chest, victim, Vec3::X, 0.1, false);
    h.tick(1);
    h.world().despawn(victim);
    h.tick(40);
    assert!(h.global::<SchoolImpact>().is_empty());
}
