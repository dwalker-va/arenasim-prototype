//! Unit tests for the shared pre-cast guard helper used by every class AI.
//!
//! `pre_cast_ok` is the single chokepoint where friendly-CC, friendly-DoT,
//! target-immunity, spell-school lockout, silence, cooldown, and range/mana
//! checks are evaluated. These tests exercise each opt-in and the
//! silence-gating-on-mana-cost edge case in isolation.

use std::collections::BTreeMap;

use bevy::prelude::*;

use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::class_ai::cast_guard::{pre_cast_ok, PreCastOpts};
use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo};
use arenasim::states::play_match::{
    AbilityDefinitions, AbilityType, ActiveAuras, Aura, AuraType, Combatant, DRTracker, ResourceType,
    SpellSchool,
};

/// Build a minimal CombatantInfo for a target. Only fields that pre_cast_ok or
/// CombatContext queries actually touch are filled in.
fn target_info(entity: Entity, team: u8, class: CharacterClass) -> CombatantInfo {
    CombatantInfo {
        entity,
        team,
        slot: 0,
        class,
        current_health: 100.0,
        max_health: 100.0,
        current_mana: 100.0,
        max_mana: 100.0,
        position: Vec3::new(0.0, 0.0, 0.0),
        is_alive: true,
        stealthed: false,
        target: None,
        is_pet: false,
        pet_type: None,
        pet: None,
    }
}

/// Build an Aura value with sensible defaults; tests override the fields they care about.
fn make_aura(effect_type: AuraType, ability_name: &str, caster: Option<Entity>) -> Aura {
    Aura {
        effect_type,
        duration: 10.0,
        magnitude: 1.0,
        break_on_damage_threshold: -1.0,
        accumulated_damage: 0.0,
        tick_interval: 0.0,
        time_until_next_tick: 0.0,
        caster,
        ability_name: ability_name.to_string(),
        fear_direction: (0.0, 0.0),
        fear_direction_timer: 0.0,
        spell_school: None,
        applied_this_frame: false,
        backlash_damage: None,
    }
}

/// Bundle of state used by every test. Keeping them in one struct lets us
/// hand out a CombatContext that points into stable storage.
struct TestWorld {
    caster: Entity,
    caster_pos: Vec3,
    target: Entity,
    target_pos: Vec3,
    combatants: BTreeMap<Entity, CombatantInfo>,
    active_auras: BTreeMap<Entity, Vec<Aura>>,
    dr_trackers: BTreeMap<Entity, DRTracker>,
    ability_cooldowns: BTreeMap<Entity, BTreeMap<AbilityType, f32>>,
}

impl TestWorld {
    fn new(caster_class: CharacterClass) -> Self {
        let caster = Entity::from_raw(1);
        let target = Entity::from_raw(2);
        let caster_pos = Vec3::ZERO;
        let target_pos = Vec3::new(5.0, 0.0, 0.0);

        let mut combatants = BTreeMap::new();
        combatants.insert(caster, target_info(caster, 1, caster_class));
        combatants.insert(target, target_info(target, 2, CharacterClass::Mage));

        Self {
            caster,
            caster_pos,
            target,
            target_pos,
            combatants,
            active_auras: BTreeMap::new(),
            dr_trackers: BTreeMap::new(),
            ability_cooldowns: BTreeMap::new(),
        }
    }

    fn ctx(&self) -> CombatContext<'_> {
        CombatContext {
            combatants: &self.combatants,
            active_auras: &self.active_auras,
            dr_trackers: &self.dr_trackers,
            ability_cooldowns: &self.ability_cooldowns,
            self_entity: self.caster,
        }
    }
}

fn defs() -> AbilityDefinitions {
    AbilityDefinitions::default()
}

fn caster_combatant(class: CharacterClass) -> Combatant {
    let mut c = Combatant::new(1, 0, class);
    // Fully resourced caster — individual tests dial down what they need.
    c.current_mana = c.max_mana;
    c.global_cooldown = 0.0;
    c
}

// ============================================================================
// Universal checks (no opts) — sanity baseline
// ============================================================================

#[test]
fn passes_when_target_in_range_and_caster_resourced() {
    let world = TestWorld::new(CharacterClass::Warlock);
    let combatant = caster_combatant(CharacterClass::Warlock);
    let abilities = defs();
    let ability = AbilityType::Corruption;
    let def = abilities.get_unchecked(&ability);

    assert!(pre_cast_ok(
        ability,
        def,
        &combatant,
        world.caster_pos,
        None,
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts::default(),
    ));
}

#[test]
fn fails_when_caster_lacks_mana() {
    let world = TestWorld::new(CharacterClass::Warlock);
    let mut combatant = caster_combatant(CharacterClass::Warlock);
    combatant.current_mana = 0.0;
    let abilities = defs();
    let ability = AbilityType::Corruption;
    let def = abilities.get_unchecked(&ability);

    assert!(!pre_cast_ok(
        ability,
        def,
        &combatant,
        world.caster_pos,
        None,
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts::default(),
    ));
}

#[test]
fn fails_when_ability_on_cooldown() {
    let world = TestWorld::new(CharacterClass::Warlock);
    let mut combatant = caster_combatant(CharacterClass::Warlock);
    combatant.ability_cooldowns.insert(AbilityType::Fear, 5.0);
    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::Fear);

    assert!(!pre_cast_ok(
        AbilityType::Fear,
        def,
        &combatant,
        world.caster_pos,
        None,
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts::default(),
    ));
}

#[test]
fn self_targeted_ignores_range_uses_mana_check() {
    // Frost Nova doesn't pass a target into pre_cast_ok; range is enforced by callers.
    let world = TestWorld::new(CharacterClass::Mage);
    let combatant = caster_combatant(CharacterClass::Mage);
    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::FrostNova);

    assert!(pre_cast_ok(
        AbilityType::FrostNova,
        def,
        &combatant,
        world.caster_pos,
        None,
        None,
        &world.ctx(),
        PreCastOpts::default(),
    ));
}

// ============================================================================
// Friendly-breakable-CC guard (BUG-1)
// ============================================================================

#[test]
fn friendly_cc_guard_blocks_when_target_polymorphed_by_ally() {
    let mut world = TestWorld::new(CharacterClass::Warlock);
    // An ally on team 1 polymorphed the team-2 target — shouldn't blow it up.
    let ally = Entity::from_raw(3);
    world.combatants.insert(ally, target_info(ally, 1, CharacterClass::Mage));
    let mut poly = make_aura(AuraType::Polymorph, "Polymorph", Some(ally));
    poly.break_on_damage_threshold = 0.0; // breaks on any damage
    world.active_auras.insert(world.target, vec![poly]);

    let combatant = caster_combatant(CharacterClass::Warlock);
    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::Corruption);

    assert!(!pre_cast_ok(
        AbilityType::Corruption,
        def,
        &combatant,
        world.caster_pos,
        None,
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts { check_friendly_cc: true, ..Default::default() },
    ));
}

#[test]
fn friendly_cc_guard_passes_when_opt_disabled() {
    let mut world = TestWorld::new(CharacterClass::Warlock);
    let ally = Entity::from_raw(3);
    world.combatants.insert(ally, target_info(ally, 1, CharacterClass::Mage));
    let mut poly = make_aura(AuraType::Polymorph, "Polymorph", Some(ally));
    poly.break_on_damage_threshold = 0.0;
    world.active_auras.insert(world.target, vec![poly]);

    let combatant = caster_combatant(CharacterClass::Warlock);
    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::Corruption);

    // Same situation, but opt-out — Fear, for example, has its own
    // is-already-CC'd check rather than relying on the friendly-CC opt.
    assert!(pre_cast_ok(
        AbilityType::Corruption,
        def,
        &combatant,
        world.caster_pos,
        None,
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts::default(),
    ));
}

// ============================================================================
// Friendly-DoTs guard (BUG-2 / Polymorph)
// ============================================================================

#[test]
fn friendly_dots_guard_blocks_polymorph_on_dotted_target() {
    let mut world = TestWorld::new(CharacterClass::Mage);
    // Warlock teammate already has Corruption ticking on the target.
    let ally = Entity::from_raw(3);
    world.combatants.insert(ally, target_info(ally, 1, CharacterClass::Warlock));
    let dot = make_aura(AuraType::DamageOverTime, "Corruption", Some(ally));
    world.active_auras.insert(world.target, vec![dot]);

    let combatant = caster_combatant(CharacterClass::Mage);
    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::Polymorph);

    assert!(!pre_cast_ok(
        AbilityType::Polymorph,
        def,
        &combatant,
        world.caster_pos,
        None,
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts { check_friendly_dots: true, ..Default::default() },
    ));
}

// ============================================================================
// Target damage-immunity guard (Divine Shield)
// ============================================================================

#[test]
fn target_immunity_guard_blocks_when_target_has_damage_immunity() {
    let mut world = TestWorld::new(CharacterClass::Priest);
    let immune = make_aura(AuraType::DamageImmunity, "Divine Shield", None);
    world.active_auras.insert(world.target, vec![immune]);

    let combatant = caster_combatant(CharacterClass::Priest);
    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::MindBlast);

    assert!(!pre_cast_ok(
        AbilityType::MindBlast,
        def,
        &combatant,
        world.caster_pos,
        None,
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts { check_target_immune: true, ..Default::default() },
    ));
}

// ============================================================================
// Silence guard — gated on mana_cost > 0 and caster's resource type
// ============================================================================

#[test]
fn silence_blocks_mana_caster() {
    let world = TestWorld::new(CharacterClass::Priest);
    let mut combatant = caster_combatant(CharacterClass::Priest);
    assert_eq!(combatant.resource_type, ResourceType::Mana);
    let auras = ActiveAuras { auras: vec![make_aura(AuraType::Silence, "UA Backlash", None)] };
    combatant.current_mana = combatant.max_mana;

    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::FlashHeal);
    assert!(def.mana_cost > 0.0);

    assert!(!pre_cast_ok(
        AbilityType::FlashHeal,
        def,
        &combatant,
        world.caster_pos,
        Some(&auras),
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts::default(),
    ));
}

#[test]
fn silence_does_not_block_rage_user() {
    // Warriors on Rage: silence is irrelevant by design (is_silenced returns
    // false for non-Mana resource types). Verify pre_cast_ok agrees.
    let mut world = TestWorld::new(CharacterClass::Warrior);
    // Rend is melee — keep the target inside MELEE_RANGE.
    world.target_pos = Vec3::new(2.0, 0.0, 0.0);
    let mut combatant = caster_combatant(CharacterClass::Warrior);
    assert_eq!(combatant.resource_type, ResourceType::Rage);
    combatant.current_mana = combatant.max_mana; // rage is stored in current_mana
    let auras = ActiveAuras { auras: vec![make_aura(AuraType::Silence, "UA Backlash", None)] };

    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::Rend);

    assert!(pre_cast_ok(
        AbilityType::Rend,
        def,
        &combatant,
        world.caster_pos,
        Some(&auras),
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts::default(),
    ));
}

#[test]
fn bypass_silence_lets_caster_through() {
    let world = TestWorld::new(CharacterClass::Priest);
    let combatant = caster_combatant(CharacterClass::Priest);
    let auras = ActiveAuras { auras: vec![make_aura(AuraType::Silence, "UA Backlash", None)] };

    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::FlashHeal);

    assert!(pre_cast_ok(
        AbilityType::FlashHeal,
        def,
        &combatant,
        world.caster_pos,
        Some(&auras),
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts { bypass_silence: true, ..Default::default() },
    ));
}

// ============================================================================
// Spell-school lockout
// ============================================================================

#[test]
fn spell_school_lockout_blocks_matching_school() {
    let world = TestWorld::new(CharacterClass::Mage);
    let combatant = caster_combatant(CharacterClass::Mage);
    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::Frostbolt);
    assert_eq!(def.spell_school, SpellSchool::Frost);

    // Build a SpellSchoolLockout aura whose magnitude (cast as u8) matches Frost.
    // The lookup table in `is_spell_school_locked` maps magnitude=1 → Frost.
    let mut lockout = make_aura(AuraType::SpellSchoolLockout, "Pummel", None);
    lockout.magnitude = 1.0;
    let auras = ActiveAuras { auras: vec![lockout] };

    assert!(!pre_cast_ok(
        AbilityType::Frostbolt,
        def,
        &combatant,
        world.caster_pos,
        Some(&auras),
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts::default(),
    ));
}

// ============================================================================
// classify_pre_cast_failure: reason variants must match the predicate that
// actually fired in pre_cast_ok (which mirrors can_cast_config). Order matters
// — mana before range, range before min_range — so the trace doesn't lie.
// ============================================================================

use arenasim::states::play_match::class_ai::cast_guard::classify_pre_cast_failure;
use arenasim::states::play_match::decision_trace::{RejectionReason, ResourceKind};

#[test]
fn classify_returns_friendly_breakable_cc_when_opt_in_and_friendly_cc_present() {
    let world = TestWorld::new(CharacterClass::Mage);
    let mut combatant = caster_combatant(CharacterClass::Mage);
    combatant.current_mana = 100.0;
    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::Frostbolt);

    // Place friendly Polymorph on target — cast by self team
    let mut poly = make_aura(AuraType::Polymorph, "Polymorph", Some(world.caster));
    poly.break_on_damage_threshold = 0.0;
    let target_active = ActiveAuras { auras: vec![poly] };
    let mut active_auras_map = BTreeMap::new();
    active_auras_map.insert(world.target, target_active.auras.clone());

    let ctx = CombatContext {
        combatants: &world.combatants,
        active_auras: &active_auras_map,
        dr_trackers: &world.dr_trackers,
        ability_cooldowns: &world.ability_cooldowns,
        self_entity: world.caster,
    };

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    let reason = classify_pre_cast_failure(
        AbilityType::Frostbolt,
        def,
        &combatant,
        world.caster_pos,
        None,
        Some((world.target, world.target_pos)),
        &ctx,
        opts,
    );
    assert!(matches!(reason, RejectionReason::FriendlyBreakableCC), "got: {:?}", reason);
}

#[test]
fn classify_returns_insufficient_mana_before_range_for_mana_classes() {
    // can_cast_config order is mana → range → min_range. For an ability that's
    // BOTH out-of-range AND mana-short, the rejection reason must be
    // InsufficientMana (the gate that actually fires first), not OutOfRange.
    let mut world = TestWorld::new(CharacterClass::Mage);
    world.target_pos = world.caster_pos + Vec3::new(100.0, 0.0, 0.0); // way out of range
    let mut combatant = caster_combatant(CharacterClass::Mage);
    combatant.current_mana = 0.0; // also broke
    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::Frostbolt);

    let reason = classify_pre_cast_failure(
        AbilityType::Frostbolt,
        def,
        &combatant,
        world.caster_pos,
        None,
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts::default(),
    );
    assert!(
        matches!(reason, RejectionReason::InsufficientMana { .. }),
        "expected InsufficientMana (predicate order matches can_cast_config), got: {:?}",
        reason
    );
}

#[test]
fn classify_resource_kind_matches_class() {
    // Warrior uses rage (ResourceKind::Rage); Rogue uses energy.
    // Mana-class fallback returns InsufficientMana { have, need }.
    let world_warrior = TestWorld::new(CharacterClass::Warrior);
    let mut warrior = caster_combatant(CharacterClass::Warrior);
    warrior.current_mana = 0.0;
    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::MortalStrike);

    let reason = classify_pre_cast_failure(
        AbilityType::MortalStrike,
        def,
        &warrior,
        world_warrior.caster_pos,
        None,
        Some((world_warrior.target, world_warrior.target_pos)),
        &world_warrior.ctx(),
        PreCastOpts::default(),
    );
    assert!(
        matches!(reason, RejectionReason::InsufficientResource { resource: ResourceKind::Rage, .. }),
        "Warrior gets InsufficientResource{{Rage}}: {:?}",
        reason
    );

    let world_rogue = TestWorld::new(CharacterClass::Rogue);
    let mut rogue = caster_combatant(CharacterClass::Rogue);
    rogue.current_mana = 0.0;
    let def = abilities.get_unchecked(&AbilityType::SinisterStrike);

    let reason = classify_pre_cast_failure(
        AbilityType::SinisterStrike,
        def,
        &rogue,
        world_rogue.caster_pos,
        None,
        Some((world_rogue.target, world_rogue.target_pos)),
        &world_rogue.ctx(),
        PreCastOpts::default(),
    );
    assert!(
        matches!(reason, RejectionReason::InsufficientResource { resource: ResourceKind::Energy, .. }),
        "Rogue gets InsufficientResource{{Energy}}: {:?}",
        reason
    );
}

#[test]
fn classify_returns_out_of_range_when_only_range_fails() {
    let mut world = TestWorld::new(CharacterClass::Mage);
    world.target_pos = world.caster_pos + Vec3::new(100.0, 0.0, 0.0);
    let mut combatant = caster_combatant(CharacterClass::Mage);
    combatant.current_mana = 100.0; // plenty
    let abilities = defs();
    let def = abilities.get_unchecked(&AbilityType::Frostbolt);

    let reason = classify_pre_cast_failure(
        AbilityType::Frostbolt,
        def,
        &combatant,
        world.caster_pos,
        None,
        Some((world.target, world.target_pos)),
        &world.ctx(),
        PreCastOpts::default(),
    );
    assert!(
        matches!(reason, RejectionReason::OutOfRange { .. }),
        "expected OutOfRange when only range fails, got: {:?}",
        reason
    );
}
