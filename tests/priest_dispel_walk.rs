//! AS-187 — the Priest walks to a teammate in urgent crowd control that its
//! Dispel Magic cannot reach, as AS-180's Paladin does for Cleanse; and under
//! `TeamPlan` a PRESSURED healer (where the walk is retired in favour of the
//! team solve) is walked there by the solve's dispel goal.
//!
//! These cases drive the real posture tick (`evaluate_priest_posture`) and the
//! real rotation (`decide_priest_action`) over a hand-built scene, and read back
//! what they issued and traced.

use std::collections::{BTreeMap, HashSet};

use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

use arenasim::combat::log::CombatLog;
use arenasim::states::match_config::CharacterClass;
use arenasim::states::play_match::ai_profile::AiProfile;
use arenasim::states::play_match::class_ai::priest::{
    decide_priest_action, evaluate_priest_posture, PriestMovementPlan,
};
use arenasim::states::play_match::class_ai::{CombatContext, CombatantInfo};
use arenasim::states::play_match::decision_trace::DecisionTrace;
use arenasim::states::play_match::map_geometry::{
    has_line_of_sight, resolve_movement, steer_toward_goal, ObstacleVolume,
};
use arenasim::states::play_match::traps::freezing_trap_aura;
use arenasim::states::play_match::{
    AbilityDefinitions, AbilityType, ActiveAuras, Aura, Combatant, HealerPosture, MovementConfig,
    MovementDirective, MovementGoal, Posture, GCD,
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

/// The scene: the Priest at the origin, its Warrior teammate `distance` yards
/// down +X carrying `warrior_auras`, and an enemy Hunter beside the Warrior with
/// the Warrior as its target (so nothing presses the Priest itself).
struct Scene {
    world: World,
    priest: Entity,
    warrior: Entity,
    hunter: Entity,
    warrior_pos: Vec3,
    roster: BTreeMap<Entity, CombatantInfo>,
    active_auras: BTreeMap<Entity, Vec<Aura>>,
    combatant: Combatant,
    obstacles: Vec<ObstacleVolume>,
    profile: AiProfile,
    /// The posture state the next tick starts from (`None`: a fresh FREE).
    posture: Option<HealerPosture>,
}

const PRIEST_POS: Vec3 = Vec3::new(0.0, 1.0, 0.0);
const NOW: f32 = 20.0;

fn scene(distance: f32, warrior_auras: Vec<Aura>) -> Scene {
    let mut world = World::new();
    let priest = world.spawn_empty().id();
    let warrior = world.spawn_empty().id();
    let hunter = world.spawn_empty().id();

    let warrior_pos = Vec3::new(distance, 1.0, 0.0);
    let mut combatant = Combatant::new(1, 0, CharacterClass::Priest);
    combatant.target = Some(hunter);

    let mut roster = BTreeMap::new();
    let mut me = info(priest, 1, CharacterClass::Priest, PRIEST_POS);
    me.current_mana = combatant.current_mana;
    me.max_mana = combatant.max_mana;
    roster.insert(priest, me);
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
        priest,
        warrior,
        hunter,
        warrior_pos,
        roster,
        active_auras,
        combatant,
        obstacles: Vec::new(),
        profile: AiProfile::Legacy,
        posture: None,
    }
}

fn trapped() -> Vec<Aura> {
    vec![freezing_trap_aura(Entity::from_raw(99))]
}

/// A posture state held in `posture` for the whole tick: PRESSURED by its
/// hysteresis floor (nothing presses the Priest in these scenes), ESCAPE by
/// its window.
fn held(posture: Posture) -> HealerPosture {
    let mut state = HealerPosture::new(NOW - 1.0);
    state.posture = posture;
    state.hold_until = NOW + 10.0;
    state.escape_until = NOW + 10.0;
    state
}

/// The Priest's view of the scene. A free function over the fields, so the
/// rotation can borrow the Priest's `Combatant` mutably alongside it.
fn context<'a>(
    priest: Entity,
    roster: &'a BTreeMap<Entity, CombatantInfo>,
    active_auras: &'a BTreeMap<Entity, Vec<Aura>>,
    obstacles: &'a [ObstacleVolume],
    profile: AiProfile,
) -> CombatContext<'a> {
    CombatContext::new(
        priest,
        1,
        roster,
        active_auras,
        &EMPTY_DR,
        &EMPTY_CD,
        obstacles,
        Default::default(),
        profile,
    )
}

/// What one posture tick left behind.
struct Tick {
    plan: PriestMovementPlan,
    directive: Option<MovementDirective>,
    /// `(trigger, target entity)` of every movement decision traced.
    moves: Vec<(String, Option<u64>)>,
}

impl Tick {
    fn triggers(&self) -> Vec<&str> {
        self.moves.iter().map(|(t, _)| t.as_str()).collect()
    }

    fn walk_point(&self) -> Vec3 {
        match self.directive.as_ref().expect("a directive").goal {
            MovementGoal::Point(p) => p,
            other => panic!("expected a Point walk, got {other:?}"),
        }
    }
}

impl Scene {
    fn ctx(&self) -> CombatContext<'_> {
        context(
            self.priest,
            &self.roster,
            &self.active_auras,
            &self.obstacles,
            self.profile,
        )
    }

    /// One posture tick at `NOW`, from `self.posture`.
    fn posture(&mut self) -> Tick {
        let abilities = AbilityDefinitions::default();
        let movement = MovementConfig::default();
        let mut trace = DecisionTrace::default();
        let mut queue = CommandQueue::default();
        let mut state = self.posture;
        let plan = {
            let ctx = self.ctx();
            let mut commands = Commands::new(&mut queue, &self.world);
            evaluate_priest_posture(
                &mut commands,
                self.priest,
                &self.combatant,
                PRIEST_POS,
                &ctx,
                &abilities,
                None,
                state.as_mut(),
                None,
                &movement,
                NOW,
                &mut trace,
            )
        };
        queue.apply(&mut self.world);
        let directive = self.world.get::<MovementDirective>(self.priest).cloned();
        let moves = trace
            .pending_events
            .drain(..)
            .filter_map(|e| {
                let v = serde_json::to_value(&e).ok()?;
                Some((
                    v["trigger"].as_str()?.to_string(),
                    v["target"]["entity_id"].as_u64(),
                ))
            })
            .collect();
        Tick {
            plan,
            directive,
            moves,
        }
    }

    /// One rotation tick with no movement plan. Returns the chosen ability and
    /// its target.
    fn decide(&mut self) -> Option<(String, Option<u64>)> {
        let abilities = AbilityDefinitions::default();
        let movement = MovementConfig::default();
        let mut combat_log = CombatLog::default();
        let mut trace = DecisionTrace::default();
        let mut queue = CommandQueue::default();
        let mut shielded = HashSet::new();
        // Everyone already carries Fortitude (priority 1 steps aside).
        let mut fortified: HashSet<Entity> = self.roster.keys().copied().collect();
        let mut same_frame_cc = Vec::new();
        let self_auras = ActiveAuras { auras: Vec::new() };
        {
            let ctx = context(
                self.priest,
                &self.roster,
                &self.active_auras,
                &self.obstacles,
                self.profile,
            );
            let mut commands = Commands::new(&mut queue, &self.world);
            decide_priest_action(
                &mut commands,
                &mut combat_log,
                &abilities,
                self.priest,
                &mut self.combatant,
                PRIEST_POS,
                Some(&self_auras),
                &ctx,
                &mut shielded,
                &mut fortified,
                &PriestMovementPlan::default(),
                &movement,
                &mut same_frame_cc,
                &mut trace,
            );
        }
        let event = serde_json::to_value(&trace.pending_events[0]).unwrap();
        event["outcome"]["ability"]
            .as_str()
            .map(|a| (a.to_string(), event["outcome"]["target_id"].as_u64()))
    }

    fn warrior_id(&self) -> Option<u64> {
        Some(self.warrior.index() as u64)
    }
}

static EMPTY_DR: BTreeMap<Entity, arenasim::states::play_match::DRTracker> = BTreeMap::new();
static EMPTY_CD: BTreeMap<Entity, BTreeMap<AbilityType, f32>> = BTreeMap::new();

fn dispel_range() -> f32 {
    AbilityDefinitions::default()
        .get(&AbilityType::DispelMagic)
        .unwrap()
        .range
}

// ============================================================================
// Part 1: the Priest's dispel walk
// ============================================================================

/// Trapped beyond Dispel Magic's range: the Priest walks straight at the
/// Warrior, traced as a `DispelChase` naming it, and defers its non-critical
/// casts for the walk.
#[test]
fn a_teammate_trapped_out_of_range_draws_the_priest_toward_it() {
    let mut s = scene(dispel_range() + 15.0, trapped());
    let tick = s.posture();
    assert_eq!(
        tick.walk_point(),
        s.warrior_pos,
        "walks at the trapped Warrior"
    );
    assert_eq!(
        tick.moves,
        vec![("DispelChase".to_string(), s.warrior_id())]
    );
    assert!(
        tick.plan.escape_defer.is_some(),
        "a Flash Heal mid-walk would root the Priest out of range"
    );
}

/// Trapped within range: no walk, and the rotation's urgent Dispel Magic frees
/// the Warrior where the Priest stands.
#[test]
fn a_teammate_trapped_in_range_is_dispelled_without_a_walk() {
    let mut s = scene(dispel_range() - 5.0, trapped());
    let tick = s.posture();
    assert!(!tick.triggers().contains(&"DispelChase"));
    assert!(tick.plan.escape_defer.is_none());
    assert_eq!(
        s.decide(),
        Some(("DispelMagic".to_string(), s.warrior_id()))
    );
}

/// Nothing urgent on the Warrior: no walk at any distance.
#[test]
fn an_untrapped_teammate_draws_no_walk() {
    let mut s = scene(dispel_range() + 15.0, Vec::new());
    let tick = s.posture();
    assert!(!tick.triggers().contains(&"DispelChase"));
    assert!(tick.plan.escape_defer.is_none());
}

/// The walk's gate (`ally_walk_allowed`), through the Priest's own posture
/// tick, one tick per case. Under `Legacy` the Priest walks in FREE and
/// PRESSURED; under `TeamPlan` it walks in FREE, and in PRESSURED the team
/// solve takes it toward the Warrior instead (part 2, below). It never walks
/// during a committed ESCAPE window, nor while it is hard-CC'd itself.
#[test]
fn the_priest_walks_only_where_an_ally_walk_is_allowed() {
    let walk = |profile: AiProfile, posture: Option<HealerPosture>, priest_ccd: bool| {
        let mut s = scene(dispel_range() + 15.0, trapped());
        s.profile = profile;
        s.posture = posture;
        if priest_ccd {
            s.active_auras
                .insert(s.priest, vec![freezing_trap_aura(Entity::from_raw(98))]);
        }
        let tick = s.posture();
        let chased = tick
            .moves
            .contains(&("DispelChase".to_string(), s.warrior_id()));
        (chased, tick.plan.escape_defer.is_some())
    };

    use AiProfile::{Legacy, TeamPlan};
    // FREE and PRESSURED, both profiles: walked toward the Warrior, casts deferred.
    assert_eq!(walk(Legacy, None, false), (true, true), "Legacy FREE");
    assert_eq!(
        walk(Legacy, Some(held(Posture::Pressured)), false),
        (true, true),
        "Legacy PRESSURED"
    );
    assert_eq!(walk(TeamPlan, None, false), (true, true), "TeamPlan FREE");
    assert_eq!(
        walk(TeamPlan, Some(held(Posture::Pressured)), false),
        (true, true),
        "TeamPlan PRESSURED — the solve's walk"
    );
    // Never in a committed ESCAPE window. (ESCAPE defers casts on its own
    // account, so only the walk is asserted.)
    assert!(
        !walk(Legacy, Some(held(Posture::Escape)), false).0,
        "ESCAPE"
    );
    assert!(
        !walk(TeamPlan, Some(held(Posture::Escape)), false).0,
        "ESCAPE"
    );
    // Never while the Priest itself is hard-CC'd.
    assert_eq!(walk(Legacy, None, true), (false, false), "CC'd, FREE");
    assert_eq!(
        walk(TeamPlan, Some(held(Posture::Pressured)), true),
        (false, false),
        "CC'd, TeamPlan PRESSURED"
    );
}

// ============================================================================
// Part 2: the TeamPlan solve's dispel goal
// ============================================================================

/// A `TeamPlan` Priest held PRESSURED, the Warrior trapped beyond range in the
/// open: the solve sends it to the NEAREST spot from which Dispel Magic
/// reaches — along the line to the Warrior, just inside range — rather than to
/// cover or to the Warrior itself.
#[test]
fn a_pressured_teamplan_priest_is_solved_to_the_nearest_reaching_spot() {
    let range = dispel_range();
    let mut s = scene(range + 15.0, trapped());
    s.profile = AiProfile::TeamPlan;
    s.posture = Some(held(Posture::Pressured));
    let tick = s.posture();

    let p = tick.walk_point();
    let to_warrior = p.distance(s.warrior_pos);
    assert!(
        to_warrior <= range,
        "solved to {to_warrior:.1}yd, beyond {range}yd"
    );
    assert!(
        to_warrior >= range - 2.0,
        "solved to {to_warrior:.1}yd — walked further in than reach needs"
    );
    assert!(p.z.abs() < 1e-3, "off the straight line: {p:?}");
    assert_eq!(
        tick.moves,
        vec![("DispelChase".to_string(), s.warrior_id())]
    );
    assert!(tick.plan.escape_defer.is_some());
}

/// The same Priest owes nothing once the Warrior is in reach — and nothing for
/// an untrapped Warrior: no `DispelChase`, no deferral.
#[test]
fn a_pressured_teamplan_priest_owes_no_walk_without_an_unreached_trap() {
    for (distance, auras) in [
        (dispel_range() - 5.0, trapped()),
        (dispel_range() + 15.0, Vec::new()),
    ] {
        let mut s = scene(distance, auras);
        s.profile = AiProfile::TeamPlan;
        s.posture = Some(held(Posture::Pressured));
        let tick = s.posture();
        assert!(!tick.triggers().contains(&"DispelChase"), "at {distance}yd");
        assert!(tick.plan.escape_defer.is_none(), "at {distance}yd");
    }
}

/// A Priest that cannot cast Dispel Magic — silenced, or locked out of Holy —
/// owes no walk under either profile: it could not dispel on arrival, and a
/// walk that releases at the range edge with nothing to cast hands it back to
/// a posture that steps it out again, strobing there for the whole lockout.
#[test]
fn a_priest_that_cannot_cast_dispel_magic_does_not_walk() {
    use arenasim::states::play_match::{AuraType, SpellSchool};
    let silence = || {
        let mut a = freezing_trap_aura(Entity::from_raw(97));
        a.effect_type = AuraType::Silence;
        a
    };
    let holy_lock = || {
        let mut a = freezing_trap_aura(Entity::from_raw(97));
        a.effect_type = AuraType::SpellSchoolLockout;
        a.magnitude = SpellSchool::Holy.to_lockout_magnitude();
        a
    };
    for (profile, posture) in [
        (AiProfile::Legacy, None),
        (AiProfile::Legacy, Some(held(Posture::Pressured))),
        (AiProfile::TeamPlan, None),
        (AiProfile::TeamPlan, Some(held(Posture::Pressured))),
    ] {
        for (what, aura) in [("silenced", silence()), ("Holy-locked", holy_lock())] {
            let mut s = scene(dispel_range() + 15.0, trapped());
            s.profile = profile;
            s.posture = posture;
            s.active_auras.insert(s.priest, vec![aura]);
            let tick = s.posture();
            assert!(
                !tick.triggers().contains(&"DispelChase"),
                "{profile:?} {:?}: walked while {what}",
                posture.map(|p| p.posture)
            );
        }
    }
}

// ============================================================================
// The probes: the trapped Warrior is freed within the walk time
// ============================================================================

/// Where and when the stepped scene's first Dispel Magic on the Warrior landed.
struct Freed {
    /// Seconds after the first tick.
    at: f32,
    /// The Priest's position when it cast.
    from: Vec3,
    /// Every Priest position stepped through before the cast.
    path: Vec<Vec3>,
    /// Movement triggers traced along the way.
    triggers: Vec<String>,
}

/// Step the real posture tick and the real rotation at 60Hz over the scene for
/// up to `seconds`, moving the Priest along whatever `Point` directive the tick
/// left at its base speed — tangent-steered around the scene's obstacles and
/// resolved against them, the move `move_to_target` makes. Returns the first
/// Dispel Magic on the Warrior, if one lands.
fn step_until_dispelled(s: &mut Scene, seconds: f32) -> Option<Freed> {
    const DT: f32 = 1.0 / 60.0;
    let speed = s.combatant.base_movement_speed;
    let abilities = AbilityDefinitions::default();
    let movement = MovementConfig::default();
    let mut pos = PRIEST_POS;
    let mut path = Vec::new();
    let mut triggers = Vec::new();
    if let Some(p) = s.posture {
        s.world.entity_mut(s.priest).insert(p);
    }

    for tick in 0..(seconds / DT) as usize {
        let now = NOW + tick as f32 * DT;
        s.roster.get_mut(&s.priest).unwrap().position = pos;
        s.combatant.global_cooldown = (s.combatant.global_cooldown - DT).max(0.0);
        // Hold a PRESSURED start PRESSURED throughout: nothing presses the
        // Priest here, and the case is about the pressured healer.
        if let Some(mut p) = s.world.get_mut::<HealerPosture>(s.priest) {
            if p.posture == Posture::Pressured {
                p.hold_until = now + 10.0;
            }
        }

        let mut trace = DecisionTrace::default();
        let mut queue = CommandQueue::default();
        let plan = {
            let ctx = s.ctx();
            let mut posture = s.world.get::<HealerPosture>(s.priest).copied();
            let directive = s.world.get::<MovementDirective>(s.priest).cloned();
            let mut commands = Commands::new(&mut queue, &s.world);
            let plan = evaluate_priest_posture(
                &mut commands,
                s.priest,
                &s.combatant,
                pos,
                &ctx,
                &abilities,
                None,
                posture.as_mut(),
                directive.as_ref(),
                &movement,
                now,
                &mut trace,
            );
            if let Some(p) = posture {
                commands.entity(s.priest).insert(p);
            }
            plan
        };
        queue.apply(&mut s.world);

        let mut combat_log = CombatLog::default();
        let mut shielded = HashSet::new();
        let mut fortified: HashSet<Entity> = s.roster.keys().copied().collect();
        let self_auras = ActiveAuras { auras: Vec::new() };
        let mut same_frame_cc = Vec::new();
        {
            let ctx = context(
                s.priest,
                &s.roster,
                &s.active_auras,
                &s.obstacles,
                s.profile,
            );
            let mut commands = Commands::new(&mut queue, &s.world);
            decide_priest_action(
                &mut commands,
                &mut combat_log,
                &abilities,
                s.priest,
                &mut s.combatant,
                pos,
                Some(&self_auras),
                &ctx,
                &mut shielded,
                &mut fortified,
                &plan,
                &movement,
                &mut same_frame_cc,
                &mut trace,
            );
        }
        queue.apply(&mut s.world);
        let events: Vec<serde_json::Value> = trace
            .pending_events
            .iter()
            .map(|e| serde_json::to_value(e).unwrap())
            .collect();
        triggers.extend(
            events
                .iter()
                .filter_map(|v| v["trigger"].as_str().map(str::to_string)),
        );
        let dispelled = events.iter().any(|v| {
            v["outcome"]["ability"] == "DispelMagic"
                && v["outcome"]["target_id"].as_u64() == Some(s.warrior.index() as u64)
        });
        if dispelled {
            return Some(Freed {
                at: tick as f32 * DT,
                from: pos,
                path,
                triggers,
            });
        }
        path.push(pos);

        if let Some(MovementDirective {
            goal: MovementGoal::Point(p),
            ..
        }) = s.world.get::<MovementDirective>(s.priest)
        {
            let to_goal = *p - pos;
            if to_goal.length() > 1e-3 {
                let dir = steer_toward_goal(
                    &s.obstacles,
                    Vec2::new(pos.x, pos.z),
                    Vec2::new(p.x, p.z),
                    pos.y,
                )
                .map(|d| Vec3::new(d.x, 0.0, d.y))
                .unwrap_or_else(|| Vec3::new(to_goal.x, 0.0, to_goal.z).normalize_or_zero());
                let step = (dir * speed * DT).clamp_length_max(to_goal.length());
                pos = resolve_movement(&s.obstacles, pos, pos + Vec3::new(step.x, 0.0, step.z));
            }
        }
    }
    None
}

/// The Warrior Freezing-Trapped 15yd beyond Dispel Magic's range: freed within
/// the time it takes the Priest to walk those 15yd (plus, for the solve, the
/// one-yard reach margin), and never from beyond range — under `Legacy` from
/// FREE, and under `TeamPlan` from a held PRESSURED, where only the solve's
/// dispel goal can move it.
#[test]
fn a_priest_frees_a_warrior_trapped_out_of_range_within_the_walk_time() {
    let range = dispel_range();
    let gap = 15.0;
    for (profile, posture) in [
        (AiProfile::Legacy, None),
        (AiProfile::TeamPlan, Some(held(Posture::Pressured))),
    ] {
        let mut s = scene(range + gap, trapped());
        s.profile = profile;
        s.posture = posture;
        let speed = s.combatant.base_movement_speed;
        let freed = step_until_dispelled(&mut s, 8.0)
            .unwrap_or_else(|| panic!("{profile:?}: the trapped Warrior is never freed"));
        let distance = freed.from.distance(s.warrior_pos);
        let walk = (gap + 1.0) / speed;
        assert!(
            distance <= range,
            "{profile:?}: dispelled from {distance:.1}yd, beyond {range}yd"
        );
        assert!(
            freed.at <= walk + 0.1,
            "{profile:?}: freed after {:.2}s; the walk takes {walk:.2}s at {speed}yd/s",
            freed.at
        );
        assert!(
            freed.triggers.iter().any(|t| t == "DispelChase"),
            "{profile:?}: freed without a traced DispelChase"
        );
    }
}

/// How far down +X the Warrior stands in the line-of-sight scene: well inside
/// Dispel Magic's range, so range never decides it.
const IN_RANGE: f32 = 20.0;

/// The probe behind a pillar: the Warrior trapped in range with a pillar
/// between it and the Priest. Both walks round the pillar and dispel once the
/// Priest can see the Warrior — never through the pillar, and from within
/// range. Under `TeamPlan` PRESSURED no solve candidate clears the pillar at
/// first, so the solve walks straight at the Warrior and tangent steering
/// rounds it.
#[test]
fn a_priest_rounds_a_pillar_to_dispel_a_trapped_teammate() {
    let range = dispel_range();
    for (profile, posture) in [
        (AiProfile::Legacy, None),
        (AiProfile::TeamPlan, Some(held(Posture::Pressured))),
    ] {
        let mut s = scene(IN_RANGE, trapped());
        s.profile = profile;
        s.posture = posture;
        s.obstacles.push(ObstacleVolume::Cylinder {
            center_xz: Vec2::new(IN_RANGE / 2.0, 0.0),
            radius: 3.0,
            base_y: 0.0,
            height: 10.0,
        });
        assert!(!has_line_of_sight(&s.obstacles, PRIEST_POS, s.warrior_pos));

        let freed = step_until_dispelled(&mut s, 8.0).unwrap_or_else(|| {
            panic!("{profile:?}: the Priest never rounds the pillar to free the Warrior")
        });
        assert!(
            has_line_of_sight(&s.obstacles, freed.from, s.warrior_pos),
            "{profile:?}: dispelled through the pillar from {:?}",
            freed.from
        );
        assert!(freed.from.distance(s.warrior_pos) <= range);
        assert!(
            freed.triggers.iter().any(|t| t == "DispelChase"),
            "{profile:?}: freed without a traced DispelChase — something else walked it"
        );
        let first_sight = freed
            .path
            .iter()
            .position(|p| has_line_of_sight(&s.obstacles, *p, s.warrior_pos))
            .unwrap_or(freed.path.len());
        assert!(first_sight > 0, "{profile:?}: had sight from the start");
        let waited = freed.at - first_sight as f32 / 60.0;
        assert!(
            waited <= GCD + 1.0 / 60.0,
            "{profile:?}: dispelled {waited:.2}s after regaining sight"
        );
    }
}

/// The scene's enemy is never what moves the Priest: with the Warrior free and
/// the Priest held PRESSURED under `TeamPlan`, the solve stands it still in the
/// open (nothing to hide from but a Hunter it has sight of and no pillar) —
/// the control for the walks above.
#[test]
fn without_a_trap_the_pressured_teamplan_priest_does_not_walk_to_the_warrior() {
    let mut s = scene(dispel_range() + 15.0, Vec::new());
    s.profile = AiProfile::TeamPlan;
    s.posture = Some(held(Posture::Pressured));
    let _ = s.hunter;
    let tick = s.posture();
    let p = tick.walk_point();
    assert!(
        p.distance(s.warrior_pos) > dispel_range(),
        "moved into dispel reach with nothing to dispel: {p:?}"
    );
}

/// The fallback walk, stepped: the Warrior trapped behind a pillar so wide that
/// none of the solve's candidates — local steps, shadow points, shoulders, the
/// line toward the Warrior — reaches it at first. A `TeamPlan` Priest held
/// PRESSURED must still free it: the solve walks straight at the Warrior when
/// nothing reaches, and tangent steering rounds the pillar. (Without the
/// fallback every candidate ties and the stand-still tie-break wins.)
#[test]
fn a_pressured_teamplan_priest_rounds_a_wide_pillar_no_candidate_clears() {
    let range = dispel_range();
    let mut s = scene(32.0, trapped());
    s.profile = AiProfile::TeamPlan;
    s.posture = Some(held(Posture::Pressured));
    s.obstacles.push(ObstacleVolume::Cylinder {
        center_xz: Vec2::new(20.0, 0.0),
        radius: 10.0,
        base_y: 0.0,
        height: 10.0,
    });
    assert!(!has_line_of_sight(&s.obstacles, PRIEST_POS, s.warrior_pos));

    let freed = step_until_dispelled(&mut s, 8.0)
        .expect("the Priest never rounds the wide pillar to free the Warrior");
    assert!(has_line_of_sight(&s.obstacles, freed.from, s.warrior_pos));
    assert!(freed.from.distance(s.warrior_pos) <= range);
    assert!(freed.triggers.iter().any(|t| t == "DispelChase"));
}

/// The solve's walk HOLDS its reach (AS-198, the `TeamPlan` half of AS-197).
/// The Warrior trapped 15yd beyond Dispel Magic's range in the open, the enemy
/// Hunter moved off the line above the reaching spot, and a pillar whose shadow
/// from the Hunter is a fully satisfying cover spot 34yd from the Warrior —
/// inside heal range, outside dispel range. The Priest's GCD is still running
/// when it arrives, so the dispel waits; the Priest must hold reach until it
/// lands, rather than stepping back to the cover spot the moment the dispel
/// reaches and walking in again when the goal re-arms.
#[test]
fn a_pressured_teamplan_priest_holds_reach_until_the_dispel_lands() {
    let range = dispel_range();
    let mut s = scene(range + 15.0, trapped());
    s.profile = AiProfile::TeamPlan;
    s.posture = Some(held(Posture::Pressured));
    s.roster.get_mut(&s.hunter).unwrap().position = Vec3::new(16.0, 1.0, 30.0);
    s.obstacles.push(ObstacleVolume::Prism {
        center_xz: Vec2::new(8.0, 8.0),
        circumradius: 6.0,
        sides: 8,
        rotation: 0.0,
        base_y: 0.0,
        height: 5.0,
    });
    let speed = s.combatant.base_movement_speed;
    // Busy for a second past the walk: the dispel cannot land on arrival.
    let busy = (15.0 + 1.0) / speed + 1.0;
    s.combatant.global_cooldown = busy;

    let freed = step_until_dispelled(&mut s, 10.0).expect("the trapped Warrior is never freed");
    let reached = freed
        .path
        .iter()
        .position(|p| p.distance(s.warrior_pos) <= range)
        .expect("the Priest never reached the Warrior");
    let worst = freed.path[reached..]
        .iter()
        .map(|p| p.distance(s.warrior_pos))
        .fold(0.0f32, f32::max);
    assert!(
        worst <= range,
        "stepped back to {worst:.1}yd from the Warrior while the dispel waited"
    );
    assert_eq!(
        freed
            .triggers
            .iter()
            .filter(|t| *t == "DispelChase")
            .count(),
        1,
        "the walk re-armed: {:?}",
        freed.triggers
    );
    assert!(
        freed.at <= busy + 2.0 / 60.0,
        "freed {:.2}s after the GCD came back",
        freed.at - busy
    );
}

/// A DYING teammate outranks the owed dispel (the `Legacy` medic-first order).
/// The scene adds a second teammate, a Rogue 30yd behind the Priest at 20% HP;
/// the Warrior is trapped 45yd the other way, beyond Dispel Magic's range, and
/// an enemy Warrior stands 10yd from the Priest on the way. A `TeamPlan` Priest
/// held PRESSURED owes no dispel while the Rogue is dying: no `DispelChase`, and
/// it stays in heal range and sight of the Rogue rather than walking 46yd off it
/// past the enemy melee. The same scene with the Rogue healthy walks — the
/// control that the scene does owe the dispel.
#[test]
fn a_dying_teammate_outranks_the_owed_dispel_under_teamplan() {
    let heal_range = MovementConfig::default().shared.heal_range;
    let walk = |rogue_hp: f32| {
        let mut s = scene(dispel_range() + 15.0, trapped());
        s.profile = AiProfile::TeamPlan;
        s.posture = Some(held(Posture::Pressured));
        let rogue = s.world.spawn_empty().id();
        let rogue_pos = Vec3::new(-30.0, 1.0, 0.0);
        let mut r = info(rogue, 1, CharacterClass::Rogue, rogue_pos);
        r.slot = 2;
        r.current_health = rogue_hp;
        s.roster.insert(rogue, r);
        let melee = s.world.spawn_empty().id();
        let mut m = info(melee, 2, CharacterClass::Warrior, Vec3::new(10.0, 1.0, 0.0));
        m.slot = 1;
        s.roster.insert(melee, m);
        let tick = s.posture();
        let chased = tick.triggers().contains(&"DispelChase");
        (chased, tick.walk_point(), rogue_pos, s.warrior_pos)
    };

    let (chased, p, rogue_pos, _) = walk(20.0);
    assert!(!chased, "walked for the dispel while the Rogue was dying");
    assert!(
        p.distance(rogue_pos) <= heal_range,
        "solved {:.1}yd from the dying Rogue, beyond heal range {heal_range}",
        p.distance(rogue_pos)
    );

    let (chased, p, _, warrior_pos) = walk(100.0);
    assert!(
        chased,
        "the control: a healthy Rogue does not hold the walk"
    );
    assert!(p.distance(warrior_pos) <= dispel_range());
}

/// What does NOT count as "another teammate dying": the CC'd teammate itself
/// (walking to it brings it into heal range too), and a friendly pet. In both
/// scenes a `TeamPlan` Priest held PRESSURED still walks for the dispel.
#[test]
fn the_trapped_teammate_and_pets_do_not_hold_the_walk() {
    use arenasim::states::play_match::{PetType, PET_SLOT_BASE};
    let chases = |s: &mut Scene| {
        s.profile = AiProfile::TeamPlan;
        s.posture = Some(held(Posture::Pressured));
        s.posture().triggers().contains(&"DispelChase")
    };

    // The trapped Warrior is itself at 20%, and no one else is hurt.
    let mut s = scene(dispel_range() + 15.0, trapped());
    s.roster.get_mut(&s.warrior).unwrap().current_health = 20.0;
    assert!(chases(&mut s), "a dying trapped teammate held its own walk");

    // A friendly pet beside the Priest at 20%.
    let mut s = scene(dispel_range() + 15.0, trapped());
    let pet = s.world.spawn_empty().id();
    let mut p = info(pet, 1, CharacterClass::Hunter, Vec3::new(-5.0, 1.0, 0.0));
    p.slot = PET_SLOT_BASE;
    p.is_pet = true;
    p.pet_type = Some(PetType::Boar);
    p.current_health = 20.0;
    s.roster.insert(pet, p);
    assert!(chases(&mut s), "a hurt friendly pet held the walk");
}

// ============================================================================
// End to end: headless matches
// ============================================================================

/// One traced dispel walk: its posture, how far the teammate was when it began,
/// and whether a Dispel Magic from the walking Priest landed on that teammate
/// within 5s.
struct Walk {
    posture: String,
    from: f64,
    ended_in_a_dispel: bool,
}

/// Every `DispelChase` a team-1 Priest traced in `config`'s match.
fn priest_walks(config: arenasim::headless::HeadlessMatchConfig) -> Vec<Walk> {
    use arenasim::headless::run_headless_match_with;
    use arenasim::headless::runner::TraceConfig;

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();
    drop(tmp);
    run_headless_match_with(
        config,
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
    let priest = |e: &serde_json::Value| e["actor"]["class"] == "Priest" && e["actor"]["team"] == 1;

    events
        .iter()
        .filter(|e| priest(e) && e["trigger"] == "DispelChase")
        .map(|walk| {
            let (start, ally) = (
                walk["sim_time"].as_f64().unwrap(),
                walk["target"]["entity_id"].as_u64(),
            );
            let dispelled = events.iter().any(|e| {
                priest(e)
                    && e["outcome"]["ability"] == "DispelMagic"
                    && e["outcome"]["target_id"].as_u64() == ally
                    && (start..=start + 5.0).contains(&e["sim_time"].as_f64().unwrap())
            });
            Walk {
                posture: walk["posture"].as_str().unwrap_or("").to_string(),
                from: walk["target"]["distance"].as_f64().unwrap_or(0.0),
                ended_in_a_dispel: dispelled,
            }
        })
        .collect()
}

/// The walk in a real `Legacy` match: `Warrior+Priest vs Warlock+Priest`, where
/// the Warlock fears the Warrior out of Dispel Magic's range in the opening.
/// Asserted over seeds rather than pinned to one, with a floor so the probe
/// cannot pass by never walking: at least one walk (begun beyond reach by
/// construction) must end with a Dispel Magic on that same teammate, which the
/// rotation only casts once it reaches.
#[test]
fn a_priest_walks_to_a_feared_teammate_and_dispels_it() {
    use arenasim::headless::HeadlessMatchConfig;
    let mut ended_in_a_dispel = 0;
    for seed in [1, 3] {
        ended_in_a_dispel += priest_walks(HeadlessMatchConfig {
            team1: vec!["Warrior".into(), "Priest".into()],
            team2: vec!["Warlock".into(), "Priest".into()],
            max_duration_secs: 40.0,
            random_seed: Some(seed),
            ..Default::default()
        })
        .iter()
        .filter(|w| w.ended_in_a_dispel)
        .count();
    }
    assert!(
        ended_in_a_dispel >= 1,
        "no dispel walk ended in a Dispel Magic on the walked-to teammate"
    );
}

/// The walk end to end under `TeamPlan`, both sides: `Rogue+Priest vs
/// Mage+Warlock`, where the Warlock's Death Coil sends the Rogue 30-40yd off
/// while the Priest is PRESSURED. A walk traced from PRESSURED, begun beyond
/// Dispel Magic's range, ends in a Dispel Magic on the Rogue; on `main` before
/// this card the Priest never freed it in either seed. Asserted over seeds with
/// a floor of one, found by scanning both sides `TeamPlan`.
///
/// What this cannot isolate is WHICH half moved the Priest: a pressured healer
/// that relaxes to FREE mid-walk is carried the rest of the way by the
/// FREE-posture walk. The solve's own share is pinned by the stepped probe
/// above, which holds the Priest PRESSURED throughout.
#[test]
fn a_pressured_teamplan_priest_is_solved_to_an_out_of_range_teammate_and_dispels_it() {
    use arenasim::headless::HeadlessMatchConfig;
    let range = f64::from(dispel_range());
    let mut out_of_range_walks_ending_in_a_dispel = 0;
    for seed in [10, 12] {
        out_of_range_walks_ending_in_a_dispel += priest_walks(HeadlessMatchConfig {
            team1: vec!["Rogue".into(), "Priest".into()],
            team2: vec!["Mage".into(), "Warlock".into()],
            ai_profile: Some("TeamPlan".into()),
            max_duration_secs: 60.0,
            random_seed: Some(seed),
            ..Default::default()
        })
        .iter()
        .filter(|w| w.posture == "pressured" && w.from > range && w.ended_in_a_dispel)
        .count();
    }
    assert!(
        out_of_range_walks_ending_in_a_dispel >= 1,
        "no PRESSURED TeamPlan walk begun beyond range ended in a Dispel Magic"
    );
}
