//! The Druid's control and escape kit — Entangling Roots, Cyclone and the
//! Travel Form shift — pinned rule by rule.
//!
//! Two kinds of probe, as the rules need them:
//!
//! - **System probes** drive the real system (`apply_pending_auras`,
//!   `process_travel_form`, the damage and healing funnels,
//!   `decide_druid_action`) on a hand-built world, each PAIRED with a control
//!   that differs only in the rule's condition, so a rule that stops doing
//!   anything fails instead of passing vacuously. They assert aura SETS and
//!   health, never a flag.
//! - **Match probes** play real seeded Druid matches, observed frame by frame,
//!   and assert what the world shows: a shifted Druid pulling away from its
//!   chaser, a cycloned unit that neither moves nor changes health, a shifted
//!   Druid that casts nothing. Each counts its occurrences and fails if a seed
//!   shift empties them.

use std::collections::BTreeMap;
use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::headless::{run_headless_match_observed, FrameObservation, HeadlessMatchConfig};
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::auras::apply_pending_auras;
use arenasim::states::play_match::class_ai::druid::decide_druid_action;
use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo};
use arenasim::states::play_match::combat_core::{apply_damage_with_absorb, apply_healing};
use arenasim::states::play_match::components::{
    ActiveAuras, ArenaDampening, Aura, AuraPending, AuraType, CastingState, Combatant, DRTracker,
    GameRng, ShapeshiftPending, Shift,
};
use arenasim::states::play_match::decision_trace::{AbilityOutcome, DecisionTrace, EventPayload};
use arenasim::states::play_match::effects::process_travel_form;
use arenasim::states::play_match::{AbilityDefinitions, AbilityType, MovementConfig, SpellSchool};

// ── shared scaffolding ──────────────────────────────────────────────────────

fn world() -> World {
    let mut world = World::new();
    world.insert_resource(CombatLog::default());
    world.insert_resource(ArenaDampening::default());
    world.insert_resource(GameRng::from_seed(7));
    world.insert_resource(AbilityDefinitions::default());
    let mut time = Time::<()>::default();
    time.advance_by(Duration::from_secs_f32(0.0));
    world.insert_resource(time);
    world
}

fn unit(world: &mut World, team: u8, class: CharacterClass, auras: Vec<Aura>) -> Entity {
    let mut entity = world.spawn((
        Combatant::new(team, 0, class),
        Transform::default(),
        DRTracker::default(),
    ));
    if !auras.is_empty() {
        entity.insert(ActiveAuras { auras });
    }
    entity.id()
}

/// The aura `ability` lands from `caster`, exactly as the RON describes it.
fn aura_of(ability: AbilityType, target: Entity, caster: Entity) -> Aura {
    let defs = AbilityDefinitions::default();
    AuraPending::from_ability(target, caster, defs.get_unchecked(&ability))
        .expect("the ability applies an aura")
        .aura
}

fn land(world: &mut World, target: Entity, aura: Aura) {
    world.spawn(AuraPending { target, aura });
    world.run_system_once(apply_pending_auras).unwrap();
}

fn aura_types(world: &World, entity: Entity) -> Vec<AuraType> {
    world
        .entity(entity)
        .get::<ActiveAuras>()
        .map(|a| a.auras.iter().map(|aura| aura.effect_type).collect())
        .unwrap_or_default()
}

fn duration_of(world: &World, entity: Entity, ty: AuraType) -> Option<f32> {
    world
        .entity(entity)
        .get::<ActiveAuras>()
        .and_then(|a| a.auras.iter().find(|aura| aura.effect_type == ty))
        .map(|aura| aura.duration)
}

fn sorted(mut types: Vec<AuraType>) -> Vec<String> {
    let mut names: Vec<String> = types.drain(..).map(|t| format!("{t:?}")).collect();
    names.sort();
    names
}

// ── Travel Form: the shift breaks roots and snares ─────────────────────────

/// A Druid rooted by Entangling Roots (face and damage rider), snared by a
/// PHYSICAL slow no dispel could take, and carrying a DoT and a HoT, shifts.
/// Every root and snare goes — the physical one included, and the Roots'
/// damage rider with its face — and nothing else does.
#[test]
fn a_shift_breaks_every_root_and_snare_and_nothing_else() {
    let mut world = world();
    let enemy = unit(&mut world, 2, CharacterClass::Druid, vec![]);
    let druid = unit(&mut world, 1, CharacterClass::Druid, vec![]);
    land(
        &mut world,
        druid,
        aura_of(AbilityType::EntanglingRoots, druid, enemy),
    );
    land(
        &mut world,
        druid,
        aura_of(AbilityType::ConcussiveShot, druid, enemy),
    );
    land(
        &mut world,
        druid,
        aura_of(AbilityType::Corruption, druid, enemy),
    );
    land(
        &mut world,
        druid,
        aura_of(AbilityType::Rejuvenation, druid, druid),
    );
    let before = sorted(aura_types(&world, druid));
    assert_eq!(
        before,
        sorted(vec![
            AuraType::Root,
            AuraType::DamageOverTime, // the Roots rider
            AuraType::MovementSpeedSlow,
            AuraType::DamageOverTime, // Corruption
            AuraType::HealingOverTime,
        ]),
        "the scene did not build: nothing to break"
    );
    let snare = world
        .entity(druid)
        .get::<ActiveAuras>()
        .unwrap()
        .auras
        .iter()
        .find(|a| a.effect_type == AuraType::MovementSpeedSlow)
        .cloned()
        .unwrap();
    assert!(snare.is_physical() && !snare.can_be_dispelled());

    world.spawn(ShapeshiftPending {
        caster: druid,
        shift: Shift::IntoTravelForm,
    });
    world.run_system_once(process_travel_form).unwrap();

    assert_eq!(
        sorted(aura_types(&world, druid)),
        sorted(vec![
            AuraType::DamageOverTime, // Corruption stays
            AuraType::HealingOverTime,
            AuraType::TravelForm,
        ])
    );
    let corruption = world
        .entity(druid)
        .get::<ActiveAuras>()
        .unwrap()
        .auras
        .iter()
        .any(|a| a.ability_name == "Corruption");
    assert!(
        corruption,
        "the DoT left standing must be Corruption, not the Roots rider"
    );
}

/// Shifting out takes the form away, and only the form.
#[test]
fn shifting_out_removes_the_form_only() {
    let mut world = world();
    let druid = unit(&mut world, 1, CharacterClass::Druid, vec![]);
    world.spawn(ShapeshiftPending {
        caster: druid,
        shift: Shift::IntoTravelForm,
    });
    world.run_system_once(process_travel_form).unwrap();
    land(
        &mut world,
        druid,
        aura_of(AbilityType::Rejuvenation, druid, druid),
    );
    assert!(aura_types(&world, druid).contains(&AuraType::TravelForm));

    world.spawn(ShapeshiftPending {
        caster: druid,
        shift: Shift::Out,
    });
    world.run_system_once(process_travel_form).unwrap();
    assert_eq!(aura_types(&world, druid), vec![AuraType::HealingOverTime]);
}

// ── Travel Form: Polymorph does not land on a shifted Druid ────────────────

/// Paired: the same Polymorph on the same Druid, shifted and not.
#[test]
fn a_polymorph_at_a_shifted_druid_does_not_land() {
    let polymorphed = |shifted: bool| {
        let mut world = world();
        let mage = unit(&mut world, 2, CharacterClass::Mage, vec![]);
        let druid = unit(&mut world, 1, CharacterClass::Druid, vec![]);
        if shifted {
            world.spawn(ShapeshiftPending {
                caster: druid,
                shift: Shift::IntoTravelForm,
            });
            world.run_system_once(process_travel_form).unwrap();
        }
        land(
            &mut world,
            druid,
            aura_of(AbilityType::Polymorph, druid, mage),
        );
        aura_types(&world, druid).contains(&AuraType::Polymorph)
    };
    assert!(
        polymorphed(false),
        "control: the Polymorph lands on an unshifted Druid"
    );
    assert!(!polymorphed(true), "a shifted Druid is immune to Polymorph");
}

// ── Cyclone: no damage, no healing, no new aura ────────────────────────────

fn cycloned_target(cycloned: bool) -> (Combatant, ActiveAuras) {
    let mut target = Combatant::new(2, 0, CharacterClass::Rogue);
    target.current_health = 100.0;
    let mut auras = Vec::new();
    if cycloned {
        auras.push(aura_of(
            AbilityType::Cyclone,
            Entity::from_raw(1),
            Entity::from_raw(2),
        ));
    }
    (target, ActiveAuras { auras })
}

/// Paired: the same hit on the same Rogue, cycloned and not — physical and
/// magic both, through the one damage funnel.
#[test]
fn a_cycloned_target_takes_no_damage() {
    for school in [SpellSchool::Physical, SpellSchool::Nature] {
        let hit = |cycloned: bool| {
            let (mut target, mut auras) = cycloned_target(cycloned);
            let mut log = CombatLog::default();
            let (dealt, _) = apply_damage_with_absorb(
                40.0,
                &mut target,
                Some(&mut auras),
                school,
                "probe",
                &mut log,
            );
            (dealt, target.current_health)
        };
        let (dealt, health) = hit(false);
        assert!(
            dealt > 0.0 && health < 100.0,
            "control: the {school:?} hit lands"
        );
        assert_eq!(
            hit(true),
            (0.0, 100.0),
            "{school:?} damage reached a cycloned target"
        );
    }
}

/// Paired: the same heal on the same Rogue, cycloned and not.
#[test]
fn a_cycloned_target_takes_no_healing() {
    let heal = |cycloned: bool| {
        let (mut target, auras) = cycloned_target(cycloned);
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        let mut queue = CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &world);
        let healed = apply_healing(
            &mut commands,
            entity,
            &mut target,
            Some(&auras),
            &ArenaDampening::default(),
            40.0,
        );
        (healed, target.current_health)
    };
    assert_eq!(heal(false), (40.0, 140.0), "control: the heal lands");
    assert_eq!(heal(true), (0.0, 100.0), "a cycloned target was healed");
}

/// Nothing new lands on a cycloned target — a friendly shield and a hostile
/// root alike — and the same auras land on it uncycloned.
#[test]
fn nothing_lands_on_a_cycloned_target() {
    let landed = |cycloned: bool| {
        let mut world = world();
        let priest = unit(&mut world, 2, CharacterClass::Priest, vec![]);
        let druid = unit(&mut world, 1, CharacterClass::Druid, vec![]);
        let rogue = unit(&mut world, 2, CharacterClass::Rogue, vec![]);
        if cycloned {
            land(
                &mut world,
                rogue,
                aura_of(AbilityType::Cyclone, rogue, druid),
            );
        }
        land(
            &mut world,
            rogue,
            aura_of(AbilityType::PowerWordShield, rogue, priest),
        );
        land(
            &mut world,
            rogue,
            aura_of(AbilityType::EntanglingRoots, rogue, druid),
        );
        let mut types = aura_types(&world, rogue);
        types.retain(|t| *t != AuraType::Cyclone);
        sorted(types)
    };
    assert_eq!(
        landed(false),
        sorted(vec![
            AuraType::Absorb,
            AuraType::Root,
            AuraType::DamageOverTime
        ]),
        "control: both land on an uncycloned target"
    );
    assert!(
        landed(true).is_empty(),
        "{:?} landed through a Cyclone",
        landed(true)
    );
}

// ── Cyclone: its own diminishing returns ───────────────────────────────────

/// Land `first` on a fresh target until it is DR-immune (three times), then
/// land `then` and return `then`'s duration — `None` if it did not land.
fn after_three(first: AbilityType, then: AbilityType) -> Option<f32> {
    let mut world = world();
    let caster = unit(&mut world, 1, CharacterClass::Druid, vec![]);
    let target = unit(&mut world, 2, CharacterClass::Rogue, vec![]);
    let first_type = aura_of(first, target, caster).effect_type;
    for _ in 0..3 {
        land(&mut world, target, aura_of(first, target, caster));
        // Let each one end, so the next is not a replacement of the last.
        world
            .entity_mut(target)
            .get_mut::<ActiveAuras>()
            .unwrap()
            .auras
            .retain(|a| a.effect_type != first_type);
    }
    let then_aura = aura_of(then, target, caster);
    let then_type = then_aura.effect_type;
    land(&mut world, target, then_aura);
    duration_of(&world, target, then_type)
}

#[test]
fn cyclone_diminishes_apart_from_polymorph_and_fear() {
    let defs = AbilityDefinitions::default();
    let full = |a: AbilityType| {
        defs.get_unchecked(&a)
            .applies_aura
            .as_ref()
            .unwrap()
            .duration
    };

    // Control: the bucket does fill — a fourth Cyclone is refused.
    assert_eq!(
        after_three(AbilityType::Cyclone, AbilityType::Cyclone),
        None
    );
    assert_eq!(
        after_three(AbilityType::Polymorph, AbilityType::Polymorph),
        None
    );
    assert_eq!(after_three(AbilityType::Fear, AbilityType::Fear), None);

    // Neither Polymorph nor Fear touches Cyclone's bucket, and Cyclone
    // touches neither of theirs.
    let cyclone = full(AbilityType::Cyclone);
    assert_eq!(
        after_three(AbilityType::Polymorph, AbilityType::Cyclone),
        Some(cyclone)
    );
    assert_eq!(
        after_three(AbilityType::Fear, AbilityType::Cyclone),
        Some(cyclone)
    );
    assert_eq!(
        after_three(AbilityType::Cyclone, AbilityType::Polymorph),
        Some(full(AbilityType::Polymorph))
    );
    assert_eq!(
        after_three(AbilityType::Cyclone, AbilityType::Fear),
        Some(full(AbilityType::Fear))
    );
}

// ── Travel Form: the Druid casts nothing while shifted ─────────────────────

fn info(entity: Entity, team: u8, class: CharacterClass, position: Vec3) -> CombatantInfo {
    CombatantInfo {
        entity,
        team,
        slot: 0,
        class,
        current_health: 100.0,
        max_health: 100.0,
        current_mana: 100.0,
        max_mana: 100.0,
        position,
        velocity: Vec3::ZERO,
        is_alive: true,
        stealthed: false,
        target: None,
        is_pet: false,
        casting_ability: None,
        pet_type: None,
        pet: None,
    }
}

/// A dying ally in range and a Warrior in the Druid's face. Returns what the
/// Druid chose, and whether it started a cast or queued a shift.
fn decide_with_dying_ally(shifted: bool) -> (Option<AbilityType>, bool, bool) {
    decide_scene(shifted, 1.0)
}

/// The Druid, a Warrior ally at 20% HP ten yards off, and an enemy Warrior
/// `enemy_distance` yards from the Druid, attacking it.
fn decide_scene(shifted: bool, enemy_distance: f32) -> (Option<AbilityType>, bool, bool) {
    let mut world = World::new();
    let druid = world.spawn_empty().id();
    let ally = world.spawn_empty().id();
    let enemy = world.spawn_empty().id();
    let abilities = AbilityDefinitions::default();
    let movement = MovementConfig::default();
    let mut combat_log = CombatLog::default();
    let mut trace = DecisionTrace::default();

    let druid_pos = Vec3::new(0.0, 1.0, 0.0);
    let mut combatant = Combatant::new(1, 1, CharacterClass::Druid);
    combatant.target = Some(enemy);
    let mut roster = BTreeMap::new();
    roster.insert(druid, info(druid, 1, CharacterClass::Druid, druid_pos));
    let mut hurt = info(ally, 1, CharacterClass::Warrior, Vec3::new(10.0, 1.0, 0.0));
    hurt.current_health = 20.0;
    roster.insert(ally, hurt);
    let mut warrior = info(
        enemy,
        2,
        CharacterClass::Warrior,
        druid_pos + Vec3::X * enemy_distance,
    );
    warrior.target = Some(druid);
    roster.insert(enemy, warrior);

    // Mark of the Wild on both, so the pre-match buff is out of the way.
    let mark = aura_of(AbilityType::MarkOfTheWild, druid, druid);
    let mut self_auras = vec![mark.clone()];
    if shifted {
        self_auras.push(aura_of(AbilityType::TravelForm, druid, druid));
    }
    let mut active = BTreeMap::new();
    active.insert(druid, self_auras.clone());
    active.insert(ally, vec![mark]);
    let dr = BTreeMap::new();
    let cooldowns = BTreeMap::new();
    let ctx = CombatContext::new(
        druid,
        1,
        &roster,
        &active,
        &dr,
        &cooldowns,
        &[],
        Default::default(),
        Default::default(),
    );
    let live = ActiveAuras { auras: self_auras };
    let mut queue = CommandQueue::default();
    let mut commands = Commands::new(&mut queue, &world);
    decide_druid_action(
        &mut commands,
        &mut combat_log,
        &abilities,
        druid,
        &mut combatant,
        druid_pos,
        Some(&live),
        &ctx,
        &movement,
        true,
        true,
        60.0,
        &mut trace,
    );
    queue.apply(&mut world);

    let chosen = trace.pending_events.drain(..).find_map(|event| {
        let EventPayload::Ability { outcome, .. } = &event.payload else {
            return None;
        };
        match outcome {
            AbilityOutcome::ActionTaken { ability, .. } => Some(*ability),
            AbilityOutcome::NoAction { .. } => None,
        }
    });
    let casting = world.entity(druid).contains::<CastingState>();
    let mut shifts = world.query::<&ShapeshiftPending>();
    let shifting = shifts.iter(&world).next().is_some();
    (chosen, casting, shifting)
}

#[test]
fn the_druid_casts_nothing_while_shifted() {
    let (chosen, casting, _) = decide_with_dying_ally(false);
    assert!(
        chosen.is_some() && casting,
        "control: unshifted, the Druid acts on a dying ally (chose {chosen:?})"
    );
    assert_eq!(
        decide_with_dying_ally(true),
        (None, false, false),
        "shifted with a melee in its face: no cast, no action, and no shift out"
    );
}

/// Every Druid ability is refused by the shared pre-cast guard while the
/// Druid is shifted, and the same guard passes the kit's heal unshifted.
#[test]
fn the_pre_cast_guard_refuses_every_druid_ability_while_shifted() {
    use arenasim::states::play_match::class_ai::cast_guard::{pre_cast_ok, PreCastOpts};

    let abilities = AbilityDefinitions::default();
    let druid = Entity::from_raw(1);
    let ally = Entity::from_raw(2);
    let pos = Vec3::new(0.0, 1.0, 0.0);
    let ally_pos = Vec3::new(5.0, 1.0, 0.0);
    let mut roster = BTreeMap::new();
    roster.insert(druid, info(druid, 1, CharacterClass::Druid, pos));
    roster.insert(ally, info(ally, 1, CharacterClass::Warrior, ally_pos));
    let active = BTreeMap::new();
    let dr = BTreeMap::new();
    let cooldowns = BTreeMap::new();
    let ctx = CombatContext::new(
        druid,
        1,
        &roster,
        &active,
        &dr,
        &cooldowns,
        &[],
        Default::default(),
        Default::default(),
    );
    let combatant = Combatant::new(1, 1, CharacterClass::Druid);
    let form = ActiveAuras {
        auras: vec![aura_of(AbilityType::TravelForm, druid, druid)],
    };
    let ok = |ability: AbilityType, auras: Option<&ActiveAuras>| {
        pre_cast_ok(
            ability,
            abilities.get_unchecked(&ability),
            &combatant,
            pos,
            auras,
            Some((ally, ally_pos)),
            &ctx,
            PreCastOpts::default(),
        )
    };
    assert!(
        ok(AbilityType::Rejuvenation, None),
        "control: the guard passes unshifted"
    );
    let kit = abilities.abilities_for_class(CharacterClass::Druid);
    assert!(kit.len() >= 9, "the Druid kit came back short: {kit:?}");
    for ability in kit {
        assert!(
            !ok(ability, Some(&form)),
            "{ability:?} passed the guard while shifted"
        );
    }
}

/// Shifted, with no melee in reach and an ally to heal, the Druid shifts out;
/// with the melee in its face it does not (the case above).
#[test]
fn a_safe_shifted_druid_with_healing_to_do_shifts_out() {
    let (chosen, casting, shifting) = decide_shifted_with_hurt_ally(false);
    assert_eq!(
        (chosen, casting),
        (None, false),
        "the shift out is not a cast"
    );
    assert!(
        shifting,
        "safe with a hurt ally: the Druid queues its shift out"
    );
    assert!(
        !decide_shifted_with_hurt_ally(true).2,
        "a melee in reach keeps the Druid shifted"
    );
}

fn decide_shifted_with_hurt_ally(melee_in_reach: bool) -> (Option<AbilityType>, bool, bool) {
    decide_scene(true, if melee_in_reach { 1.0 } else { 25.0 })
}

// ── match probes ────────────────────────────────────────────────────────────

/// One observed Druid match: every frame, and the match log's lines.
struct Played {
    frames: Vec<FrameObservation>,
    log: String,
}

fn play(team1: &[&str], team2: &[&str], map: &str, seed: u64) -> Played {
    let dir = tempfile::tempdir().expect("tempdir");
    let log_path = dir.path().join("match.txt");
    let config = HeadlessMatchConfig {
        team1: team1.iter().map(|s| s.to_string()).collect(),
        team2: team2.iter().map(|s| s.to_string()).collect(),
        map: map.to_string(),
        max_duration_secs: 120.0,
        random_seed: Some(seed),
        output_path: Some(log_path.to_string_lossy().to_string()),
        ..Default::default()
    };
    let mut frames = Vec::new();
    run_headless_match_observed(config, false, None, |f| frames.push(f.clone()))
        .expect("match runs");
    let log = std::fs::read_to_string(&log_path).expect("match log written");
    Played { frames, log }
}

/// `[  44.23s] ...` → 44.23. Log time and `sim_time` share a clock.
fn log_time(line: &str) -> Option<f32> {
    let open = line.find('[')?;
    let close = line.find("s]")?;
    line[open + 1..close].trim().parse().ok()
}

fn log_id(team: u8, slot: u8, class: CharacterClass) -> String {
    format!("Team {team} {class:?} #{}", slot + 1)
}

fn xz(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

/// The matches the smoke run found shifts out of roots and snares in, and
/// Cyclones in — fixed seeds, so each probe below has its occurrences.
const SHIFT_MATCHES: &[(&[&str], &[&str], &str, u64)] = &[
    (&["Hunter", "Druid"], &["Hunter", "Shaman"], "BasicArena", 1),
    (&["Hunter", "Druid"], &["Hunter", "Shaman"], "PillaredArena", 1),
    (&["Rogue", "Druid"], &["Rogue", "Mage"], "BasicArena", 1),
    (&["Warrior", "Druid"], &["Mage", "Paladin"], "PillaredArena", 1),
    (&["Warrior", "Druid"], &["Hunter", "Shaman"], "PillaredArena", 1),
    (&["Rogue", "Druid"], &["Hunter", "Shaman"], "PillaredArena", 1),
    (&["Rogue", "Druid"], &["Rogue", "Priest"], "BasicArena", 1),
    (&["Mage", "Druid"], &["Rogue", "Priest"], "BasicArena", 1),
    (&["Warlock", "Druid"], &["Rogue", "Mage"], "BasicArena", 1),
    (&["Hunter", "Druid"], &["Mage", "Paladin"], "BasicArena", 1),
];
const CYCLONE_MATCHES: &[(&[&str], &[&str], &str, u64)] = &[
    (&["Warrior", "Druid"], &["Rogue", "Priest"], "BasicArena", 1),
    (&["Warrior", "Druid"], &["Rogue", "Mage"], "BasicArena", 1),
];

/// How long after a shift the chase is measured.
const OUTRUN_WINDOW_SECS: f32 = 1.5;

/// A shift out of a root or snare frees the Druid and it pulls away: in the
/// window after the shift, the gap to the nearest melee or pet chaser GROWS,
/// and the Druid's path is longer than its unshifted speed could cover.
#[test]
fn a_shift_frees_the_druid_and_it_outruns_the_chaser() {
    let mut shifts = 0;
    for (t1, t2, map, seed) in SHIFT_MATCHES {
        let played = play(t1, t2, map, *seed);
        let druid_of = |f: &FrameObservation| {
            f.combatants
                .iter()
                .find(|(_, c)| c.class == CharacterClass::Druid && !c.is_pet && c.team == 1)
                .map(|(e, c)| (*e, c.clone()))
        };
        for pair in played.frames.windows(2) {
            let (Some((druid, before)), Some((_, after))) =
                (druid_of(&pair[0]), druid_of(&pair[1]))
            else {
                continue;
            };
            let impaired = |types: &[AuraType]| {
                types
                    .iter()
                    .any(|t| matches!(t, AuraType::Root | AuraType::MovementSpeedSlow))
            };
            let shifted_now = after.aura_types.contains(&AuraType::TravelForm)
                && !before.aura_types.contains(&AuraType::TravelForm);
            if !(shifted_now && impaired(&before.aura_types)) {
                continue;
            }
            assert!(
                !impaired(&after.aura_types),
                "{t1:?} v {t2:?} {map} #{seed} at {:.2}s: the shift left {:?}",
                pair[1].sim_time,
                after.aura_types
            );

            // The chaser: the nearest living melee or pet enemy.
            let chaser = pair[1]
                .combatants
                .iter()
                .filter(|(_, c)| c.team != 1 && c.alive && (c.is_pet || c.class.is_melee()))
                .min_by(|a, b| {
                    let d = |c: &arenasim::headless::ObservedCombatant| {
                        xz(c.position).distance(xz(after.position))
                    };
                    d(a.1).partial_cmp(&d(b.1)).unwrap()
                })
                .map(|(e, _)| *e);
            let Some(chaser) = chaser else { continue };

            let start = pair[1].sim_time;
            let end_frame = played
                .frames
                .iter()
                .filter(|f| f.sim_time >= start && f.sim_time <= start + OUTRUN_WINDOW_SECS)
                .take_while(|f| {
                    f.combatants[&druid].alive
                        && f.combatants[&druid]
                            .aura_types
                            .contains(&AuraType::TravelForm)
                        && f.combatants[&chaser].alive
                })
                .last()
                .unwrap();
            let elapsed = end_frame.sim_time - start;
            if elapsed < OUTRUN_WINDOW_SECS - 0.05 {
                continue; // the window was cut short by a death or a shift out
            }
            let gap = |f: &FrameObservation| {
                xz(f.combatants[&druid].position).distance(xz(f.combatants[&chaser].position))
            };
            assert!(
                gap(end_frame) > gap(&pair[1]),
                "{t1:?} v {t2:?} {map} #{seed} at {start:.2}s: the gap to the chaser went \
                 {:.1} -> {:.1}",
                gap(&pair[1]),
                gap(end_frame)
            );
            // Path length, frame by frame: a Druid sliding along a wall is
            // still moving at its full shifted speed.
            let travelled: f32 = played
                .frames
                .iter()
                .filter(|f| f.sim_time >= start && f.sim_time <= end_frame.sim_time)
                .map(|f| xz(f.combatants[&druid].position))
                .collect::<Vec<_>>()
                .windows(2)
                .map(|w| w[0].distance(w[1]))
                .sum();
            let base = Combatant::new(1, 0, CharacterClass::Druid).base_movement_speed;
            assert!(
                travelled > base * elapsed * 1.05,
                "{t1:?} v {t2:?} {map} #{seed} at {start:.2}s: {travelled:.1} yd in \
                 {elapsed:.2}s is no faster than an unshifted Druid"
            );
            shifts += 1;
        }
    }
    eprintln!("{shifts} shifts out of a root or snare, each with a full window");
    assert!(
        shifts >= 6,
        "only {shifts} shifts out of a root or snare with a full window — the seeds moved"
    );
}

/// A cycloned unit cannot act: it does not move, its health does not change
/// — no damage, no healing — and it casts nothing for as long as it spins.
#[test]
fn a_cycloned_unit_is_frozen_out_of_the_fight() {
    let mut windows = 0;
    for (t1, t2, map, seed) in CYCLONE_MATCHES {
        let played = play(t1, t2, map, *seed);
        // Each unit's runs of consecutive cycloned frames.
        let mut runs: BTreeMap<Entity, Vec<Vec<usize>>> = BTreeMap::new();
        for (i, f) in played.frames.iter().enumerate() {
            for (e, c) in &f.combatants {
                if !c.aura_types.contains(&AuraType::Cyclone) {
                    continue;
                }
                let entry = runs.entry(*e).or_default();
                match entry.last_mut() {
                    Some(run) if *run.last().unwrap() + 1 == i => run.push(i),
                    _ => entry.push(vec![i]),
                }
            }
        }
        for (e, unit_runs) in runs {
            for run in unit_runs {
                let first = &played.frames[run[0]];
                let last = &played.frames[*run.last().unwrap()];
                if last.sim_time - first.sim_time < 1.0 {
                    continue;
                }
                let (a, b) = (&first.combatants[&e], &last.combatants[&e]);
                assert_eq!(
                    xz(a.position),
                    xz(b.position),
                    "a cycloned unit moved ({t1:?} v {t2:?} #{seed})"
                );
                assert_eq!(
                    a.current_health, b.current_health,
                    "a cycloned unit's health changed ({t1:?} v {t2:?} #{seed})"
                );
                let id = log_id(a.team, a.slot, a.class);
                let acted = played.log.lines().any(|l| {
                    log_time(l).is_some_and(|t| t > first.sim_time && t < last.sim_time)
                        && (l.contains(&format!("{id} casts"))
                            || l.contains(&format!("{id} uses"))
                            || l.contains(&format!("{id} begins casting"))
                            || l.contains(&format!("{id}'s Auto Attack")))
                });
                assert!(
                    !acted,
                    "{id} acted inside its Cyclone ({t1:?} v {t2:?} #{seed})"
                );
                windows += 1;
            }
        }
    }
    eprintln!("{windows} Cyclone windows");
    assert!(
        windows >= 3,
        "only {windows} Cyclone windows — the seeds moved"
    );
}

/// While shifted, the Druid casts nothing: no cast line of its own falls
/// strictly inside a Travel Form window, in real matches.
#[test]
fn a_shifted_druid_casts_nothing_in_a_match() {
    let mut windows = 0;
    for (t1, t2, map, seed) in SHIFT_MATCHES {
        let played = play(t1, t2, map, *seed);
        let druid = |f: &FrameObservation| {
            f.combatants
                .values()
                .find(|c| c.class == CharacterClass::Druid && !c.is_pet && c.team == 1)
                .cloned()
        };
        let mut start: Option<f32> = None;
        for f in &played.frames {
            let Some(d) = druid(f) else { continue };
            let shifted = d.alive && d.aura_types.contains(&AuraType::TravelForm);
            match (start, shifted) {
                (None, true) => start = Some(f.sim_time),
                (Some(s), false) => {
                    let id = log_id(d.team, d.slot, d.class);
                    let cast = played.log.lines().find(|l| {
                        log_time(l).is_some_and(|t| t > s + 0.05 && t < f.sim_time - 0.05)
                            && (l.contains(&format!("{id} casts"))
                                || l.contains(&format!("{id} begins casting")))
                    });
                    assert!(
                        cast.is_none(),
                        "{t1:?} v {t2:?} {map} #{seed}: cast while shifted: {cast:?}"
                    );
                    windows += 1;
                    start = None;
                }
                _ => {}
            }
        }
    }
    eprintln!("{windows} Travel Form windows");
    assert!(
        windows >= 20,
        "only {windows} Travel Form windows — the seeds moved"
    );
}

