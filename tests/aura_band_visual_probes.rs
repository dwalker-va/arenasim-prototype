//! Probes for the aura-application band (`rendering/effects/aura_band.rs`) —
//! the shared cue that ends the silence of every aura whose application no
//! bespoke effect draws.
//!
//! Two halves:
//!
//! - **Routing** — which aura types reach the band, pinned as SET EQUALITY over
//!   `AuraType::ALL` (a floor would hide a type quietly falling out), plus the
//!   config sweep: every ability the gap audit found silent now routes to the
//!   band, and so does every totem's aura.
//! - **Geometry** — the band as it is DRAWN, in world space: on the unit, clear
//!   of the body, all the way round, rising for a buff and pressing down for a
//!   debuff, following the bearer, gone when its life is spent. And the refresh
//!   rule: a totem's per-pulse refresh never re-fires it; a fresh landing does.

use std::collections::BTreeSet;
use std::time::Duration;

use bevy::pbr::NotShadowCaster;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use arenasim::states::match_config::RoguePoison;
use arenasim::states::play_match::abilities::{AbilityType, SpellSchool};
use arenasim::states::play_match::ability_config::AbilityDefinitions;
use arenasim::states::play_match::class_ai::shaman::totem_spec;
use arenasim::states::play_match::components::{
    weapon_poison_marker_aura, ActiveAuras, Aura, AuraApplyOwner, AuraApplyRoute, AuraBand,
    AuraBandPolarity, AuraType, Combatant, DeferredAuraFamily, DispelType, Pet, PetType,
    TotemElement,
};
use arenasim::states::play_match::{
    cleanup_aura_bands, detect_aura_applications, update_aura_bands, AuraBandArc, AuraBandMaterial,
    AURA_BAND_ARCS, AURA_BAND_SECS, AURA_BAND_WIDTH,
};
use arenasim::CharacterClass;

const TICK: Duration = Duration::from_millis(16);
/// A combatant's capsule: `Capsule3d::new(0.5, 1.5)` centred on the transform.
const BODY_RADIUS: f32 = 0.5;
const HALF_HEIGHT: f32 = 1.25;
/// The arena floor is world y = 0; combatants stand with their transform at 1.0.
const FLOOR_Y: f32 = 0.0;
const STAND_Y: f32 = 1.0;
/// A pet's capsule: radius 0.35, cylinder 0.6, lying HORIZONTAL, so it reaches
/// 0.65 out from its centre along its length. Its mesh centre is world y 0.3.
const PET_HALF_LENGTH: f32 = 0.65;
const PET_MESH_WORLD_Y: f32 = 0.3;
const PET_RADIUS: f32 = 0.35;
/// Where `spawn_pet` puts a pet's transform (so the shared -0.45 body
/// correction lands the body centre on the mesh at 0.3).
const PET_STAND_Y: f32 = 0.75;

// =============================================================================
// Routing
// =============================================================================

/// The family: every aura type whose application the band draws.
const BAND_TYPES: &[AuraType] = &[
    AuraType::MaxHealthIncrease,
    AuraType::MaxManaIncrease,
    AuraType::AttackPowerIncrease,
    AuraType::AttackPowerReduction,
    AuraType::DamageTakenReduction,
    AuraType::SpellResistanceBuff,
    AuraType::CritChanceIncrease,
    AuraType::ManaRegenIncrease,
    AuraType::LockoutDurationReduction,
    AuraType::FrostArmorBuff,
    AuraType::WeaponPoison,
    AuraType::SpellPowerIncrease,
    AuraType::HealingOverTime,
    AuraType::WindfuryBuff,
];

/// The deliberately undrawn: the slow family, left to its own card.
const DEFERRED_TYPES: &[AuraType] = &[AuraType::MovementSpeedSlow, AuraType::AttackSpeedSlow];

fn names(types: impl IntoIterator<Item = AuraType>) -> BTreeSet<String> {
    types.into_iter().map(|t| format!("{t:?}")).collect()
}

#[test]
fn the_band_serves_exactly_the_family() {
    let routed = names(
        AuraType::ALL
            .into_iter()
            .filter(|t| AuraApplyRoute::for_aura(*t) == AuraApplyRoute::Band),
    );
    assert_eq!(
        routed,
        names(BAND_TYPES.iter().copied()),
        "the aura types routed to the shared band changed — decide each one on purpose"
    );
}

#[test]
fn only_the_slow_family_is_undrawn() {
    let undrawn = names(
        AuraType::ALL
            .into_iter()
            .filter(|t| !AuraApplyRoute::for_aura(*t).is_drawn()),
    );
    assert_eq!(undrawn, names(DEFERRED_TYPES.iter().copied()));
    for t in DEFERRED_TYPES {
        assert_eq!(
            AuraApplyRoute::for_aura(*t),
            AuraApplyRoute::Deferred(DeferredAuraFamily::Slow)
        );
    }
}

#[test]
fn every_bespoke_owner_is_one_that_exists() {
    // Each owned type names the effect that draws it. Pinning the pairs means
    // a type cannot drift to a different owner (or to the band) unnoticed.
    use AuraApplyOwner::*;
    let expected: &[(AuraType, AuraApplyOwner)] = &[
        (AuraType::Root, HardCc),
        (AuraType::Stun, HardCc),
        (AuraType::Fear, FearShroud),
        (AuraType::Polymorph, Polymorph),
        (AuraType::Incapacitate, IceBlock),
        (AuraType::Absorb, ShieldBubble),
        (AuraType::DamageImmunity, ShieldBubble),
        (AuraType::WeakenedSoul, ShieldBubble),
        (AuraType::FearImmunity, BerserkMask),
        (AuraType::DamageReduction, CurseApparition),
        (AuraType::CastTimeIncrease, CurseApparition),
        (AuraType::DamageOverTime, DotLayer),
        (AuraType::HealingReduction, MortalWounds),
        (AuraType::SpellSchoolLockout, InterruptSputter),
        (AuraType::Silence, BacklashBurst),
        (AuraType::ShadowSight, ShadowSightOrb),
    ];
    let owned = names(
        AuraType::ALL
            .into_iter()
            .filter(|t| matches!(AuraApplyRoute::for_aura(*t), AuraApplyRoute::Owned(_))),
    );
    assert_eq!(owned, names(expected.iter().map(|(t, _)| *t)));
    for (t, owner) in expected {
        assert_eq!(AuraApplyRoute::for_aura(*t), AuraApplyRoute::Owned(*owner));
    }
    // Band + owned + deferred partition the whole enum.
    assert_eq!(
        BAND_TYPES.len() + expected.len() + DEFERRED_TYPES.len(),
        AuraType::ALL.len()
    );
}

/// The abilities the animation-gap audit found silent because their aura's
/// application drew nothing. (Heroic Strike, the thirteenth, applies no aura.)
const SILENT_AURA_ABILITIES: &[AbilityType] = &[
    AbilityType::ArcaneIntellect,
    AbilityType::FrostArmor,
    AbilityType::MageArmorSpell,
    AbilityType::MoltenArmor,
    AbilityType::PowerWordFortitude,
    AbilityType::BattleShout,
    AbilityType::DemoralizingShout,
    AbilityType::CommandingShout,
    AbilityType::DevotionAura,
    AbilityType::ShadowResistanceAura,
    AbilityType::ConcentrationAura,
];

#[test]
fn every_formerly_silent_aura_ability_reaches_the_band() {
    let defs = AbilityDefinitions::default();
    for ability in SILENT_AURA_ABILITIES {
        let def = defs.get(ability).expect("ability in config");
        let aura = def
            .applies_aura
            .as_ref()
            .unwrap_or_else(|| panic!("{ability:?} applies an aura"))
            .aura_type;
        assert_eq!(
            AuraApplyRoute::for_aura(aura),
            AuraApplyRoute::Band,
            "{ability:?} applies {aura:?}, which must reach the band"
        );
    }
}

#[test]
fn every_totem_landing_and_the_poison_coating_reach_the_band() {
    for element in TotemElement::ALL {
        let (_, aura, _, _) = totem_spec(element);
        assert_eq!(
            AuraApplyRoute::for_aura(aura),
            AuraApplyRoute::Band,
            "{element:?} totem's {aura:?} landing must reach the band"
        );
    }
    let coating = weapon_poison_marker_aura(RoguePoison::Crippling);
    assert_eq!(
        AuraApplyRoute::for_aura(coating.effect_type),
        AuraApplyRoute::Band
    );
}

#[test]
fn polarity_follows_the_sims_hostility() {
    assert_eq!(
        AuraBandPolarity::of(AuraType::AttackPowerReduction),
        AuraBandPolarity::Debuff
    );
    for t in BAND_TYPES {
        let expected = if t.is_hostile_effect() {
            AuraBandPolarity::Debuff
        } else {
            AuraBandPolarity::Buff
        };
        assert_eq!(AuraBandPolarity::of(*t), expected);
    }
}

// =============================================================================
// Geometry harness
// =============================================================================

struct Harness {
    app: App,
}

fn aura(effect_type: AuraType, name: &str, school: Option<SpellSchool>) -> Aura {
    Aura {
        effect_type,
        duration: 30.0,
        magnitude: 1.0,
        break_on_damage_threshold: -1.0,
        accumulated_damage: 0.0,
        tick_interval: 0.0,
        time_until_next_tick: 0.0,
        caster: None,
        ability_name: name.to_string(),
        fear_direction: (0.0, 0.0),
        fear_direction_timer: 0.0,
        spell_school: school,
        applied_this_frame: false,
        backlash_damage: None,
        dr_category_override: None,
        dispel_type: DispelType::Auto,
        compound: None,
    }
}

fn fortitude() -> Aura {
    aura(
        AuraType::MaxHealthIncrease,
        "Power Word: Fortitude",
        Some(SpellSchool::Holy),
    )
}

fn demoralizing() -> Aura {
    aura(AuraType::AttackPowerReduction, "Demoralizing Shout", None)
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
        app.insert_resource(TimeUpdateStrategy::ManualDuration(TICK));
        app.add_systems(
            Update,
            (
                detect_aura_applications,
                update_aura_bands,
                cleanup_aura_bands,
            )
                .chain(),
        );
        // Prime Time so the first real tick advances by TICK.
        app.update();
        Harness { app }
    }

    fn tick(&mut self, frames: u32) {
        for _ in 0..frames {
            self.app.update();
        }
    }

    fn spawn_unit(&mut self, at: Vec3) -> Entity {
        self.app
            .world_mut()
            .spawn((
                Combatant::new(1, 0, CharacterClass::Priest),
                Transform::from_translation(at),
            ))
            .id()
    }

    fn spawn_pet(&mut self, at: Vec3) -> Entity {
        let owner = self.spawn_unit(Vec3::new(50.0, STAND_Y, 50.0));
        self.app
            .world_mut()
            .spawn((
                Combatant::new(1, 1, CharacterClass::Hunter),
                Pet {
                    owner,
                    pet_type: PetType::Boar,
                },
                Transform::from_translation(at),
            ))
            .id()
    }

    fn set_auras(&mut self, unit: Entity, auras: Vec<Aura>) {
        self.app
            .world_mut()
            .entity_mut(unit)
            .insert(ActiveAuras { auras });
    }

    fn clear_auras(&mut self, unit: Entity) {
        self.app
            .world_mut()
            .entity_mut(unit)
            .remove::<ActiveAuras>();
    }

    fn bands(&mut self) -> Vec<(Entity, AuraBand)> {
        let mut q = self.app.world_mut().query::<(Entity, &AuraBand)>();
        q.iter(self.app.world())
            .map(|(e, b)| (e, b.clone()))
            .collect()
    }

    /// World positions of every arc of every live band.
    fn arcs(&mut self) -> Vec<Vec3> {
        let mut q = self
            .app
            .world_mut()
            .query_filtered::<&GlobalTransform, With<AuraBandArc>>();
        q.iter(self.app.world()).map(|g| g.translation()).collect()
    }

    fn move_unit(&mut self, unit: Entity, to: Vec3) {
        self.app
            .world_mut()
            .entity_mut(unit)
            .get_mut::<Transform>()
            .unwrap()
            .translation = to;
    }
}

fn frames_for(secs: f32) -> u32 {
    (secs / TICK.as_secs_f32()).ceil() as u32
}

/// Mean height of the band's arcs — the ring's world height.
fn ring_height(arcs: &[Vec3]) -> f32 {
    arcs.iter().map(|p| p.y).sum::<f32>() / arcs.len() as f32
}

// =============================================================================
// Geometry
// =============================================================================

#[test]
fn a_buff_lands_one_band_ringing_the_body_all_the_way_round() {
    let mut h = Harness::new();
    let at = Vec3::new(3.0, STAND_Y, -2.0);
    let unit = h.spawn_unit(at);
    h.set_auras(unit, vec![fortitude()]);
    h.tick(frames_for(AURA_BAND_SECS * 0.4));

    let bands = h.bands();
    assert_eq!(bands.len(), 1, "one application, one band");
    assert_eq!(bands[0].1.target, unit);

    let arcs = h.arcs();
    assert_eq!(arcs.len(), AURA_BAND_ARCS as usize);
    // Every arc sits OUTSIDE the body with its whole band width — a ring
    // inside the 0.5 capsule is buried and depth-rejected (the dispel
    // ribbon's first-build lesson).
    for p in &arcs {
        let r = Vec2::new(p.x - at.x, p.z - at.z).length();
        assert!(
            r - AURA_BAND_WIDTH * 0.5 > BODY_RADIUS + 0.1,
            "arc at radius {r:.3} must clear the body"
        );
        assert!(r < 1.3, "arc at radius {r:.3} has drifted off the unit");
    }
    // The ring goes all the way round: the arcs' bearings leave no gap wider
    // than one arc's span (a clump beside the unit would pass a radius check).
    let mut bearings: Vec<f32> = arcs
        .iter()
        .map(|p| (p.z - at.z).atan2(p.x - at.x))
        .collect();
    bearings.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let span = std::f32::consts::TAU / AURA_BAND_ARCS as f32;
    let mut widest = bearings[0] + std::f32::consts::TAU - bearings[bearings.len() - 1];
    for w in bearings.windows(2) {
        widest = widest.max(w[1] - w[0]);
    }
    assert!(
        widest < span * 1.05,
        "widest gap {widest:.3} rad between arcs; the ring must close"
    );
}

#[test]
fn a_buff_rises_from_the_feet_past_the_crown() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, STAND_Y, 0.0);
    let unit = h.spawn_unit(at);
    h.set_auras(unit, vec![fortitude()]);
    h.tick(1);
    let start = ring_height(&h.arcs());
    let mut last = start;
    let mut peak = start;
    for _ in 0..frames_for(AURA_BAND_SECS * 0.95) {
        h.tick(1);
        let arcs = h.arcs();
        if arcs.is_empty() {
            break;
        }
        let y = ring_height(&arcs);
        assert!(
            y >= last - 1e-4,
            "a buff band never sinks ({last:.3} -> {y:.3})"
        );
        last = y;
        peak = peak.max(y);
    }
    assert!(
        start < at.y - HALF_HEIGHT + 0.6 && start > FLOOR_Y,
        "a buff starts at the feet, above the floor (started at {start:.3})"
    );
    assert!(
        peak > at.y + HALF_HEIGHT - 0.1,
        "a buff reaches the crown (peaked at {peak:.3})"
    );
}

#[test]
fn a_debuff_presses_down_from_the_crown_to_the_feet() {
    let mut h = Harness::new();
    let at = Vec3::new(0.0, STAND_Y, 0.0);
    let unit = h.spawn_unit(at);
    h.set_auras(unit, vec![demoralizing()]);
    h.tick(1);
    assert_eq!(h.bands()[0].1.polarity, AuraBandPolarity::Debuff);
    let start = ring_height(&h.arcs());
    let mut last = start;
    let mut floor = start;
    for _ in 0..frames_for(AURA_BAND_SECS * 0.95) {
        h.tick(1);
        let arcs = h.arcs();
        if arcs.is_empty() {
            break;
        }
        let y = ring_height(&arcs);
        assert!(
            y <= last + 1e-4,
            "a debuff band never climbs ({last:.3} -> {y:.3})"
        );
        last = y;
        floor = floor.min(y);
    }
    assert!(
        start > at.y + HALF_HEIGHT - 0.1,
        "starts at the crown ({start:.3})"
    );
    assert!(
        floor < at.y - HALF_HEIGHT + 0.6 && floor > FLOOR_Y,
        "presses down to the feet, above the floor ({floor:.3})"
    );
}

#[test]
fn a_totem_refresh_never_refires_but_a_fresh_landing_does() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, STAND_Y, 0.0));
    let totem = |duration: f32| {
        let mut a = aura(
            AuraType::SpellPowerIncrease,
            "Flametongue Totem",
            Some(SpellSchool::Fire),
        );
        a.duration = duration;
        a
    };
    h.set_auras(unit, vec![totem(2.0)]);
    h.tick(1);
    let first: BTreeSet<Entity> = h.bands().into_iter().map(|(e, _)| e).collect();
    assert_eq!(first.len(), 1);

    // Two seconds of pulses: the totem resets the duration every frame.
    let mut seen = first.clone();
    for i in 0..frames_for(2.0) {
        h.set_auras(unit, vec![totem(2.0 - (i % 3) as f32 * 0.1)]);
        h.tick(1);
        seen.extend(h.bands().into_iter().map(|(e, _)| e));
    }
    assert_eq!(seen, first, "a refresh is not an application");

    // The ally leaves long enough for the buff to lapse, then walks back in.
    h.clear_auras(unit);
    h.tick(3);
    h.set_auras(unit, vec![totem(2.0)]);
    h.tick(1);
    assert_eq!(h.bands().len(), 1, "a fresh landing gets a fresh band");
}

#[test]
fn many_landings_in_one_frame_read_as_one_band_per_polarity() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, STAND_Y, 0.0));
    h.set_auras(
        unit,
        vec![
            fortitude(),
            aura(AuraType::AttackPowerIncrease, "Battle Shout", None),
            aura(
                AuraType::DamageTakenReduction,
                "Devotion Aura",
                Some(SpellSchool::Holy),
            ),
            demoralizing(),
        ],
    );
    h.tick(1);
    let mut polarities: Vec<AuraBandPolarity> =
        h.bands().into_iter().map(|(_, b)| b.polarity).collect();
    polarities.sort_by_key(|p| *p == AuraBandPolarity::Debuff);
    assert_eq!(
        polarities,
        vec![AuraBandPolarity::Buff, AuraBandPolarity::Debuff]
    );
}

#[test]
fn owned_and_deferred_applications_raise_no_band() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, STAND_Y, 0.0));
    let others: Vec<Aura> = AuraType::ALL
        .into_iter()
        .filter(|t| AuraApplyRoute::for_aura(*t) != AuraApplyRoute::Band)
        .map(|t| aura(t, "anything", Some(SpellSchool::Frost)))
        .collect();
    assert!(!others.is_empty());
    h.set_auras(unit, others);
    h.tick(3);
    assert!(
        h.bands().is_empty(),
        "the band must not double a bespoke effect"
    );
}

#[test]
fn a_corpse_gets_no_band() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, STAND_Y, 0.0));
    h.app
        .world_mut()
        .entity_mut(unit)
        .get_mut::<Combatant>()
        .unwrap()
        .current_health = 0.0;
    h.set_auras(unit, vec![fortitude()]);
    h.tick(2);
    assert!(h.bands().is_empty());
}

#[test]
fn the_band_follows_a_moving_bearer() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, STAND_Y, 0.0));
    h.set_auras(unit, vec![fortitude()]);
    h.tick(2);
    let to = Vec3::new(6.0, STAND_Y, 4.0);
    h.move_unit(unit, to);
    h.tick(2);
    for p in h.arcs() {
        let r = Vec2::new(p.x - to.x, p.z - to.z).length();
        assert!(
            r > BODY_RADIUS && r < 1.3,
            "arc at {p:?} is not ringing the unit at its new position"
        );
    }
}

#[test]
fn the_band_is_gone_when_its_life_is_spent_or_its_bearer_is() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, STAND_Y, 0.0));
    h.set_auras(unit, vec![fortitude()]);
    h.tick(frames_for(AURA_BAND_SECS) + 2);
    assert!(
        h.bands().is_empty() && h.arcs().is_empty(),
        "spent bands clean up with their arcs"
    );

    let other = h.spawn_unit(Vec3::new(4.0, STAND_Y, 0.0));
    h.set_auras(other, vec![fortitude()]);
    h.tick(1);
    assert_eq!(h.bands().len(), 1);
    h.app.world_mut().entity_mut(other).despawn();
    h.tick(2);
    assert!(
        h.bands().is_empty() && h.arcs().is_empty(),
        "an orphaned band cleans up"
    );
}

#[test]
fn a_pets_band_stays_on_the_pet_and_clears_its_body() {
    let mut h = Harness::new();
    let at = Vec3::new(-3.0, PET_STAND_Y, 1.0);
    let pet = h.spawn_pet(at);
    h.set_auras(
        pet,
        vec![aura(AuraType::AttackPowerIncrease, "Battle Shout", None)],
    );
    let mut lowest = f32::MAX;
    let mut highest = f32::MIN;
    for _ in 0..frames_for(AURA_BAND_SECS * 0.95) {
        h.tick(1);
        for p in h.arcs() {
            let r = Vec2::new(p.x - at.x, p.z - at.z).length();
            // The pet lies horizontal, so the ring must clear its LENGTH.
            assert!(
                r - AURA_BAND_WIDTH * 0.5 > PET_HALF_LENGTH,
                "arc at radius {r:.3} cuts into the pet"
            );
            lowest = lowest.min(p.y);
            highest = highest.max(p.y);
        }
    }
    assert!(
        lowest > FLOOR_Y,
        "a pet's band stays above the floor ({lowest:.3})"
    );
    assert!(
        highest > PET_MESH_WORLD_Y + PET_RADIUS,
        "a pet's band clears the top of the pet ({highest:.3})"
    );
    assert!(
        highest < PET_MESH_WORLD_Y + 1.0,
        "a pet's band stays on the pet, not at a person's height ({highest:.3})"
    );
}

#[test]
fn the_band_is_additive_and_casts_no_shadow() {
    let mut h = Harness::new();
    let unit = h.spawn_unit(Vec3::new(0.0, STAND_Y, 0.0));
    h.set_auras(unit, vec![fortitude()]);
    h.tick(3);
    let handle = {
        let mut q = h.app.world_mut().query::<&AuraBandMaterial>();
        q.single(h.app.world()).unwrap().0.clone()
    };
    let materials = h.app.world().resource::<Assets<StandardMaterial>>();
    let mat = materials.get(&handle).unwrap();
    assert!(matches!(mat.alpha_mode, AlphaMode::Add));
    assert!(mat.base_color.alpha() > 0.0, "the band is visible mid-life");

    let mut q = h
        .app
        .world_mut()
        .query_filtered::<Has<NotShadowCaster>, With<AuraBandArc>>();
    assert!(q.iter(h.app.world()).all(|has| has));
}
