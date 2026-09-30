//! AS-180 — the Paladin frees a Freezing-Trapped teammate it can only reach by
//! walking.
//!
//! Cleanse has a 30yd range, and the rotation's urgent Cleanse only scans allies
//! inside it. Before this card nothing moved the Paladin toward a trapped
//! teammate, so a trap that sprang beyond 30yd ran its full 8s: in `H+Pri vs
//! Paladin+Warrior` seed 1 on the pre-AS-125 main the Warrior sat trapped 63yd
//! away while the Paladin hard-cast Flash of Light, its Cleanse rejected with a
//! bare `NoValidTarget`.
//!
//! These cases drive the real posture tick (`evaluate_paladin_posture`) and the
//! real rotation (`decide_paladin_action`) over a hand-built scene, one tick
//! each, and read back what they issued and traced.

use std::collections::{BTreeMap, HashSet};

use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::class_ai::paladin::{
    decide_paladin_action, evaluate_paladin_posture, PaladinMovementPlan,
};
use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo};
use arenasim::states::play_match::decision_trace::DecisionTrace;
use arenasim::states::play_match::traps::freezing_trap_aura;
use arenasim::states::play_match::{
    AbilityDefinitions, AbilityType, ActiveAuras, Aura, Combatant, HealerPosture, MovementConfig,
    MovementDirective, MovementGoal,
};

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

/// The scene: the Paladin at the origin, its Warrior teammate `distance` yards
/// down +X carrying `warrior_auras`, and an enemy Hunter beside the Warrior with
/// the Warrior as its target (so nothing presses the Paladin itself).
struct Scene {
    world: World,
    paladin: Entity,
    warrior: Entity,
    warrior_pos: Vec3,
    roster: BTreeMap<Entity, CombatantInfo>,
    active_auras: BTreeMap<Entity, Vec<Aura>>,
    combatant: Combatant,
}

const PALADIN_POS: Vec3 = Vec3::new(0.0, 1.0, 0.0);

fn scene(distance: f32, warrior_auras: Vec<Aura>) -> Scene {
    let mut world = World::new();
    let paladin = world.spawn_empty().id();
    let warrior = world.spawn_empty().id();
    let hunter = world.spawn_empty().id();

    let warrior_pos = Vec3::new(distance, 1.0, 0.0);
    let mut combatant = Combatant::new(1, 0, CharacterClass::Paladin);
    combatant.target = Some(hunter);

    let mut roster = BTreeMap::new();
    let mut me = info(paladin, 1, CharacterClass::Paladin, PALADIN_POS);
    me.current_mana = combatant.current_mana;
    me.max_mana = combatant.max_mana;
    roster.insert(paladin, me);
    roster.insert(
        warrior,
        info(warrior, 1, CharacterClass::Warrior, warrior_pos),
    );
    let mut enemy = info(
        hunter,
        2,
        CharacterClass::Hunter,
        warrior_pos + Vec3::new(5.0, 0.0, 0.0),
    );
    enemy.target = Some(warrior);
    roster.insert(hunter, enemy);

    let mut active_auras = BTreeMap::new();
    active_auras.insert(warrior, warrior_auras);

    Scene {
        world,
        paladin,
        warrior,
        warrior_pos,
        roster,
        active_auras,
        combatant,
    }
}

fn trapped() -> Vec<Aura> {
    vec![freezing_trap_aura(Entity::from_raw(99))]
}

/// The Paladin's view of the scene. A free function over the fields, so the
/// rotation can borrow the Paladin's `Combatant` mutably alongside it.
fn context<'a>(
    paladin: Entity,
    roster: &'a BTreeMap<Entity, CombatantInfo>,
    active_auras: &'a BTreeMap<Entity, Vec<Aura>>,
) -> CombatContext<'a> {
    CombatContext::new(
        paladin,
        1,
        roster,
        active_auras,
        &EMPTY_DR,
        &EMPTY_CD,
        &[],
        Default::default(),
        Default::default(),
    )
}

impl Scene {
    fn ctx(&self) -> CombatContext<'_> {
        context(self.paladin, &self.roster, &self.active_auras)
    }

    /// One posture tick. Returns the plan, the directive it left on the
    /// Paladin, and the movement triggers it traced.
    fn posture(&mut self) -> (PaladinMovementPlan, Option<MovementDirective>, Vec<String>) {
        let abilities = AbilityDefinitions::default();
        let movement = MovementConfig::default();
        let mut trace = DecisionTrace::default();
        let mut queue = CommandQueue::default();
        let plan = {
            let ctx = self.ctx();
            let mut commands = Commands::new(&mut queue, &self.world);
            evaluate_paladin_posture(
                &mut commands,
                &abilities,
                self.paladin,
                &self.combatant,
                PALADIN_POS,
                None,
                &ctx,
                None,
                None,
                &movement,
                20.0,
                &mut trace,
            )
        };
        queue.apply(&mut self.world);
        let directive = self.world.get::<MovementDirective>(self.paladin).cloned();
        let triggers = trace
            .pending_events
            .drain(..)
            .filter_map(|e| {
                serde_json::to_value(&e).ok()?["trigger"]
                    .as_str()
                    .map(str::to_string)
            })
            .collect();
        (plan, directive, triggers)
    }

    /// One rotation tick with no movement plan. Returns the Cleanse candidate
    /// as traced (`status`, `reason`) and the chosen ability and its target.
    fn decide(&mut self) -> (serde_json::Value, Option<(String, Option<u64>)>) {
        let abilities = AbilityDefinitions::default();
        let mut combat_log = CombatLog::default();
        let mut trace = DecisionTrace::default();
        let mut queue = CommandQueue::default();
        // The Paladin's own aura is already up (priority 1 steps aside).
        let mut aura_up: HashSet<Entity> = [self.paladin].into_iter().collect();
        let mut same_frame_cc = Vec::new();
        let self_auras = ActiveAuras { auras: Vec::new() };
        {
            let ctx = context(self.paladin, &self.roster, &self.active_auras);
            let mut commands = Commands::new(&mut queue, &self.world);
            decide_paladin_action(
                &mut commands,
                &mut combat_log,
                &abilities,
                self.paladin,
                &mut self.combatant,
                PALADIN_POS,
                Some(&self_auras),
                &ctx,
                &mut aura_up,
                &mut same_frame_cc,
                &PaladinMovementPlan::default(),
                &mut trace,
            );
        }
        let event = serde_json::to_value(&trace.pending_events[0]).unwrap();
        let cleanse = event["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["ability"] == "PaladinCleanse")
            .cloned()
            .unwrap_or_default();
        let chosen = event["outcome"]["ability"]
            .as_str()
            .map(|a| (a.to_string(), event["outcome"]["target_id"].as_u64()));
        (cleanse, chosen)
    }
}

static EMPTY_DR: BTreeMap<Entity, arenasim::states::play_match::DRTracker> = BTreeMap::new();
static EMPTY_CD: BTreeMap<Entity, BTreeMap<AbilityType, f32>> = BTreeMap::new();

fn cleanse_range() -> f32 {
    AbilityDefinitions::default()
        .get(&AbilityType::PaladinCleanse)
        .unwrap()
        .range
}

/// Trapped beyond Cleanse's range: the Paladin walks straight at the Warrior,
/// traced as a `DispelChase`, and defers its non-critical heals for the walk.
#[test]
fn a_teammate_trapped_out_of_range_draws_the_paladin_toward_it() {
    let mut s = scene(cleanse_range() + 15.0, trapped());
    let (plan, directive, triggers) = s.posture();

    let directive = directive.expect("the walk issues a directive");
    match directive.goal {
        MovementGoal::Point(p) => assert_eq!(p, s.warrior_pos, "walks at the trapped Warrior"),
        other => panic!("expected a Point walk at the Warrior, got {other:?}"),
    }
    assert_eq!(triggers, vec!["DispelChase".to_string()]);
    assert!(
        plan.cast_defer.is_some(),
        "a Flash of Light mid-walk would root the Paladin out of range"
    );
}

/// ...and the rotation says WHY it has not cleansed yet: out of range, with the
/// distance, where it used to say there was nothing to cleanse.
#[test]
fn an_out_of_range_trap_is_traced_as_out_of_range() {
    let range = cleanse_range();
    let mut s = scene(range + 15.0, trapped());
    let (cleanse, _) = s.decide();
    assert_eq!(cleanse["status"], "rejected");
    let reason = &cleanse["reason"]["OutOfRange"];
    assert_eq!(reason["max"].as_f64().unwrap() as f32, range);
    let distance = reason["distance"]
        .as_f64()
        .expect("OutOfRange carries the distance");
    assert!((distance as f32 - (range + 15.0)).abs() < 0.01);
}

/// Trapped within range: no walk (the FREE Paladin keeps its melee pursuit),
/// and the rotation's urgent Cleanse frees the Warrior where the Paladin stands.
#[test]
fn a_teammate_trapped_in_range_is_cleansed_without_a_walk() {
    let mut s = scene(cleanse_range() - 5.0, trapped());
    let (plan, directive, triggers) = s.posture();
    assert!(directive.is_none(), "no walk when Cleanse already reaches");
    assert!(triggers.iter().all(|t| t != "DispelChase"));
    assert!(plan.cast_defer.is_none());

    let (_, chosen) = s.decide();
    assert_eq!(
        chosen,
        Some(("PaladinCleanse".to_string(), Some(s.warrior.index() as u64)))
    );
}

/// Nothing urgent on the Warrior: no walk at any distance.
#[test]
fn an_untrapped_teammate_draws_no_walk() {
    let mut s = scene(cleanse_range() + 15.0, Vec::new());
    let (plan, directive, triggers) = s.posture();
    assert!(directive.is_none());
    assert!(triggers.is_empty());
    assert!(plan.cast_defer.is_none());
}

// ============================================================================
// The probe: the trapped Warrior is freed within the walk time
// ============================================================================

/// Paladin + Warrior, the Warrior Freezing-Trapped 15yd beyond Cleanse's range:
/// the Warrior is freed within the time it takes the Paladin to walk those
/// 15yd, and never from beyond range.
///
/// Steps the real posture tick and the real rotation at 60Hz over the scene,
/// moving the Paladin along whatever directive the tick left at its base speed
/// — the straight-line move `move_to_target` makes toward a `Point` goal on an
/// obstacle-free map. (A trap springing beyond Cleanse's range next to a
/// Paladin is rare on today's main, because the Hunter's trap AI aims at the
/// teammates nobody can free; this pins the Paladin's half without waiting on
/// a seed to produce it.)
#[test]
fn a_paladin_frees_a_warrior_trapped_out_of_range_within_the_walk_time() {
    const DT: f32 = 1.0 / 60.0;
    let range = cleanse_range();
    let gap = 15.0;
    let mut s = scene(range + gap, trapped());
    let speed = s.combatant.base_movement_speed;
    let abilities = AbilityDefinitions::default();
    let movement = MovementConfig::default();
    let mut pos = PALADIN_POS;

    let mut freed_at = None;
    for tick in 0..(8.0 / DT) as usize {
        let now = 20.0 + tick as f32 * DT;
        s.roster.get_mut(&s.paladin).unwrap().position = pos;
        s.combatant.global_cooldown = (s.combatant.global_cooldown - DT).max(0.0);

        let mut trace = DecisionTrace::default();
        let mut queue = CommandQueue::default();
        let plan = {
            let ctx = s.ctx();
            let mut posture = s.world.get::<HealerPosture>(s.paladin).copied();
            let directive = s.world.get::<MovementDirective>(s.paladin).cloned();
            let mut commands = Commands::new(&mut queue, &s.world);
            let plan = evaluate_paladin_posture(
                &mut commands,
                &abilities,
                s.paladin,
                &s.combatant,
                pos,
                None,
                &ctx,
                posture.as_mut(),
                directive.as_ref(),
                &movement,
                now,
                &mut trace,
            );
            if let Some(p) = posture {
                commands.entity(s.paladin).insert(p);
            }
            plan
        };
        queue.apply(&mut s.world);

        let mut combat_log = CombatLog::default();
        let mut aura_up: HashSet<Entity> = [s.paladin].into_iter().collect();
        let self_auras = ActiveAuras { auras: Vec::new() };
        let mut same_frame_cc = Vec::new();
        {
            let ctx = context(s.paladin, &s.roster, &s.active_auras);
            let mut commands = Commands::new(&mut queue, &s.world);
            decide_paladin_action(
                &mut commands,
                &mut combat_log,
                &abilities,
                s.paladin,
                &mut s.combatant,
                pos,
                Some(&self_auras),
                &ctx,
                &mut aura_up,
                &mut same_frame_cc,
                &plan,
                &mut trace,
            );
        }
        queue.apply(&mut s.world);
        let cleansed = trace.pending_events.iter().any(|e| {
            let v = serde_json::to_value(e).unwrap();
            v["outcome"]["ability"] == "PaladinCleanse"
                && v["outcome"]["target_id"].as_u64() == Some(s.warrior.index() as u64)
        });
        if cleansed {
            freed_at = Some((tick as f32 * DT, pos.distance(s.warrior_pos)));
            break;
        }

        if let Some(MovementDirective {
            goal: MovementGoal::Point(p),
            ..
        }) = s.world.get::<MovementDirective>(s.paladin)
        {
            let step = (*p - pos).normalize_or_zero() * speed * DT;
            pos += Vec3::new(step.x, 0.0, step.z);
        }
    }

    let (t, distance) = freed_at.expect("the trapped Warrior is freed before the trap ends");
    let walk = gap / speed;
    assert!(
        distance <= range,
        "cleansed from {distance:.1}yd, beyond Cleanse's {range}yd"
    );
    assert!(
        t <= walk + 0.1,
        "freed after {t:.2}s; the {gap}yd walk takes {walk:.2}s at {speed}yd/s"
    );
}

// ============================================================================
// End to end: a headless match
// ============================================================================

/// The walk in a real match. In `Hunter+Priest vs Rogue+Paladin` the Priest's
/// Psychic Scream fears the Rogue away from its Paladin, beyond Cleanse's
/// range; the Paladin walks and cleanses it. Asserted over seeds rather than
/// pinned to one, with a floor so the probe cannot pass by never walking: at
/// least one walk (a `DispelChase` run toward an ally, begun beyond range by
/// construction) must end with a Cleanse on that same ally, which the rotation
/// only casts from within range.
#[test]
fn a_paladin_walks_to_a_feared_teammate_and_cleanses_it() {
    use arenasim::headless::runner::TraceConfig;
    use arenasim::headless::{run_headless_match_with, HeadlessMatchConfig};

    let mut walks_ending_in_a_cleanse = 0;
    for seed in [1, 3] {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        drop(tmp);
        run_headless_match_with(
            HeadlessMatchConfig {
                team1: vec!["Hunter".into(), "Priest".into()],
                team2: vec!["Rogue".into(), "Paladin".into()],
                max_duration_secs: 40.0,
                random_seed: Some(seed),
                ..Default::default()
            },
            true,
            Some(TraceConfig {
                output_path: path.clone(),
            }),
        )
        .expect("headless match");
        let events: Vec<serde_json::Value> = std::fs::read_to_string(&path)
            .expect("read trace")
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();

        for walk in events.iter().filter(|e| e["trigger"] == "DispelChase") {
            let (start, ally) = (
                walk["sim_time"].as_f64().unwrap(),
                walk["target"]["entity_id"].as_u64(),
            );
            let cleansed = events.iter().any(|e| {
                e["actor"]["class"] == "Paladin"
                    && e["outcome"]["ability"] == "PaladinCleanse"
                    && e["outcome"]["target_id"].as_u64() == ally
                    && (start..=start + 5.0).contains(&e["sim_time"].as_f64().unwrap())
            });
            if cleansed {
                walks_ending_in_a_cleanse += 1;
            }
        }
    }
    assert!(
        walks_ending_in_a_cleanse >= 1,
        "no dispel walk ended in a Cleanse on the walked-to ally"
    );
}
