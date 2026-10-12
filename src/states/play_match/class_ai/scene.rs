//! Scene tests: a constructed situation, one real class decision, and a typed
//! view of what it chose and why.
//!
//! A [`Scene`] is tier 1 of the AI testing pyramid
//! (`docs/design/proposals/2026-10-11-ai-architecture.md` §3.2). It holds
//! units — class, team, position, health, resource, auras, cooldowns, posture
//! state — plus the map's obstacles and the AI profile, and
//! [`Scene::decide`] runs **the same entry point `decide_abilities` runs**,
//! [`dispatch_class_decision`], for one unit for one tick: the classification
//! of each team's situation, the unit's movement pre-pass, and its class's
//! priority chain. It returns a [`DecisionView`] read from the decision
//! trace: the chosen ability and target, every candidate with its typed
//! rejection reason, the movement decisions, and the situation the unit saw.
//!
//! A scene has no seed. No class DECISION draws from `GameRng` — only
//! execution (damage and crit rolls) does, and a scene asserts on the
//! decision. The roster is derived from the units
//! ([`CombatantInfo::from_combatant`], the snapshot's own derivation), so the
//! `Combatant` and its snapshot row cannot disagree.
//!
//! **Write every scene test as a pair**: the situation, and its control with
//! the situation's one condition removed. A paired scene cannot pass
//! vacuously — if the rule stops firing, the pair collapses.
//!
//! ```ignore
//! let mut s = Scene::new()
//!     .unit(WARRIOR, 1, Warrior).rage(80.0).targeting(MAGE)
//!     .unit(MAGE, 2, Mage).at(2.0, 0.0)
//!     .unit(PRIEST, 2, Priest).at(0.0, 20.0)
//!     .aura(PRIEST, AuraType::Polymorph, 6.0);
//! assert_eq!(s.decide(WARRIOR).chose(), Some((AbilityType::MortalStrike, Some(MAGE))));
//! s.clear_auras(PRIEST);
//! assert_eq!(s.decide(WARRIOR).chose(), Some((AbilityType::Rend, Some(MAGE))));
//! ```
//!
//! Public (and `#[doc(hidden)]`) so `tests/` can build scenes; nothing in the
//! game uses it.

use std::collections::BTreeMap;

use bevy::ecs::world::CommandQueue;
use bevy::prelude::*;

use super::combat_snapshot::CombatSnapshot;
use super::CombatantInfo;
use crate::combat::log::CombatLog;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::{load_ability_definitions, AbilityDefinitions};
use crate::states::play_match::ai_profile::{AiProfile, AiProfiles};
use crate::states::play_match::arena_bounds::ArenaBounds;
use crate::states::play_match::combat_ai::{
    dispatch_class_decision, DispatchAccumulators, DispatchFrame, DispatchIo,
};
use crate::states::play_match::components::{
    ActiveAuras, Aura, AuraType, Combatant, DRTracker, GameRng, HealerPosture, KitePosture,
    MeleeResetState, MovementDirective, RogueFlareState, Trap, TrapLaunchProjectile, TrapType,
};
use crate::states::play_match::decision_trace::{
    AbilityCandidate, AbilityOutcome, CandidateStatus, DecisionTrace, EventKind, EventPayload,
    MovementTrigger, Posture, RejectionReason, SituationTag,
};
use crate::states::play_match::map_geometry::ObstacleVolume;
use crate::states::play_match::movement_config::{load_movement_config, MovementConfig};
use crate::states::play_match::situation::{
    update_situations, MatchClock, SituationConfig, Situations,
};
use crate::states::play_match::utils::is_incapacitating;

/// A unit's name in a scene. Names, not indices, so an assertion reads as the
/// situation it describes and a failure names the unit.
pub type Who = &'static str;

/// Every scene unit stands at this height, as spawned combatants do.
const UNIT_Y: f32 = 1.0;

struct SceneUnit {
    who: Who,
    entity: Entity,
    combatant: Combatant,
    /// Named target, resolved when the scene decides — so a unit may target
    /// one added after it.
    target: Option<Who>,
    position: Vec3,
    auras: Vec<Aura>,
    healer_posture: Option<HealerPosture>,
    kite_posture: Option<KitePosture>,
    directive: Option<MovementDirective>,
    reset: Option<MeleeResetState>,
    flare: Option<RogueFlareState>,
    /// The unit's diminishing-returns state.
    dr: DRTracker,
    /// The ability the unit is casting or channelling, as its snapshot row
    /// reports it.
    casting: Option<AbilityType>,
}

/// A constructed situation. See the module docs.
#[doc(hidden)]
pub struct Scene {
    /// Owns the units' entity ids. A decision's `Commands` run in a
    /// throwaway world with the same ids, never this one: a scene asserts on
    /// what was decided, and a second `decide` sees the same scene.
    world: World,
    units: Vec<SceneUnit>,
    obstacles: Vec<ObstacleVolume>,
    bounds: ArenaBounds,
    ai_profile: AiProfiles,
    abilities: AbilityDefinitions,
    movement: MovementConfig,
    /// The sim clock: the posture clock, the trace's `sim_time` and the
    /// seconds since the gates alike.
    now: f32,
    gates_opened: bool,
    /// Each Shaman's live totem durations, by `TotemElement::index()`.
    totems: BTreeMap<Who, [f32; 4]>,
    /// The previous frame's classification, which a latched situation (tempo)
    /// and a window's onset carry over from. Empty — no history — until
    /// [`Scene::reclassify`] runs.
    situations: Situations,
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene {
    /// An empty scene under the shipped `abilities.ron` and `movement.ron`,
    /// with the gates open, on an obstacle-free map, under `Legacy`.
    pub fn new() -> Self {
        Self::with_config(
            load_ability_definitions().expect("abilities.ron loads"),
            load_movement_config().expect("movement.ron loads"),
        )
    }

    /// An empty scene, as [`Scene::new`], under the given configs. A grid of
    /// thousands of scenes loads the shipped files once and passes them here,
    /// rather than parsing both RON files per scene.
    pub fn with_config(abilities: AbilityDefinitions, movement: MovementConfig) -> Self {
        Scene {
            world: World::new(),
            units: Vec::new(),
            obstacles: Vec::new(),
            bounds: ArenaBounds::default(),
            ai_profile: AiProfiles::default(),
            abilities,
            movement,
            now: 30.0,
            gates_opened: true,
            totems: BTreeMap::new(),
            situations: Situations::default(),
        }
    }

    // ------------------------------------------------------------------
    // Building: `unit` adds a unit; the unlabelled modifiers that follow it
    // (`at`, `health`, `mana`, `rage`, `targeting`) apply to it.
    // ------------------------------------------------------------------

    /// Add a unit at full health and resource, standing at the origin, its
    /// slot the next free one on its team.
    pub fn unit(mut self, who: Who, team: u8, class: CharacterClass) -> Self {
        assert!(
            self.units.iter().all(|u| u.who != who),
            "scene already has a unit named {who}"
        );
        let slot = self
            .units
            .iter()
            .filter(|u| u.combatant.team == team)
            .count() as u8;
        let entity = self.world.spawn_empty().id();
        self.units.push(SceneUnit {
            who,
            entity,
            combatant: Combatant::new(team, slot, class),
            target: None,
            position: Vec3::new(0.0, UNIT_Y, 0.0),
            auras: Vec::new(),
            healer_posture: None,
            kite_posture: None,
            directive: None,
            reset: None,
            flare: None,
            dr: DRTracker::default(),
            casting: None,
        });
        self
    }

    fn last(&mut self) -> &mut SceneUnit {
        self.units.last_mut().expect("add a unit first")
    }

    /// The last unit's position on the ground plane.
    pub fn at(mut self, x: f32, z: f32) -> Self {
        self.last().position = Vec3::new(x, UNIT_Y, z);
        self
    }

    /// The last unit's health, as a fraction of its maximum.
    pub fn health(mut self, fraction: f32) -> Self {
        let c = &mut self.last().combatant;
        c.current_health = c.max_health * fraction;
        self
    }

    /// The last unit's mana (or energy), as a fraction of its maximum.
    pub fn mana(mut self, fraction: f32) -> Self {
        let c = &mut self.last().combatant;
        c.current_mana = c.max_mana * fraction;
        self
    }

    /// The last unit's rage, in points (a Warrior's resource is its mana pool).
    pub fn rage(mut self, points: f32) -> Self {
        self.last().combatant.current_mana = points;
        self
    }

    /// The last unit's target, by name (it may be added later).
    pub fn targeting(mut self, whom: Who) -> Self {
        self.last().target = Some(whom);
        self
    }

    /// `who`'s target, by name.
    pub fn target(mut self, who: Who, whom: Who) -> Self {
        self.unit_mut(who).target = Some(whom);
        self
    }

    /// An aura of `effect` on `who` with `duration` seconds left.
    pub fn aura(self, who: Who, effect: AuraType, duration: f32) -> Self {
        self.aura_full(
            who,
            Aura {
                effect_type: effect,
                duration,
                ..Default::default()
            },
        )
    }

    /// `aura` on `who`, as given.
    pub fn aura_full(mut self, who: Who, aura: Aura) -> Self {
        self.unit_mut(who).auras.push(aura);
        self
    }

    /// `ability` on cooldown for `who`, with `secs` left.
    pub fn cooldown(mut self, who: Who, ability: AbilityType, secs: f32) -> Self {
        self.unit_mut(who)
            .combatant
            .ability_cooldowns
            .insert(ability, secs);
        self
    }

    /// The map's obstacle volumes.
    pub fn obstacles(mut self, obstacles: Vec<ObstacleVolume>) -> Self {
        self.obstacles = obstacles;
        self
    }

    /// The AI implementation `team` runs.
    pub fn ai_profile(mut self, team: u8, profile: AiProfile) -> Self {
        match team {
            1 => self.ai_profile.team1 = profile,
            _ => self.ai_profile.team2 = profile,
        }
        self
    }

    /// The sim clock at the decision (default 30s, well after the gates). It
    /// is the seconds since the gates too, so it sets the match phase.
    pub fn now(mut self, secs: f32) -> Self {
        self.now = secs;
        self
    }

    /// The seconds left on each of `who`'s live totems, indexed by
    /// `TotemElement::index()` (`0.0`: none of that element is down).
    pub fn totems(mut self, who: Who, durations: [f32; 4]) -> Self {
        self.find(who);
        self.totems.insert(who, durations);
        self
    }

    /// `who`'s diminishing-returns state (default: nothing diminished).
    pub fn dr_tracker(mut self, who: Who, tracker: DRTracker) -> Self {
        self.unit_mut(who).dr = tracker;
        self
    }

    /// `who` is casting (or channelling) `ability`, as the snapshot row of a
    /// unit with a live `CastingState` reports it. A casting unit does not
    /// decide (`decide_abilities` skips it), so this is for the units the
    /// decision reads. Inside the dispatch only the Rogue's Kidney Shot
    /// planner reads it; the interrupts themselves are decided by
    /// `check_interrupts`, outside the dispatch, so no scene chooses one.
    pub fn casting(mut self, who: Who, ability: AbilityType) -> Self {
        self.unit_mut(who).casting = Some(ability);
        self
    }

    /// The healer posture state `who` carries into the decision.
    pub fn healer_posture(mut self, who: Who, posture: HealerPosture) -> Self {
        self.unit_mut(who).healer_posture = Some(posture);
        self
    }

    // ------------------------------------------------------------------
    // Changing a built scene, for the control half of a pair.
    // ------------------------------------------------------------------

    /// Remove every aura on `who`.
    pub fn clear_auras(&mut self, who: Who) {
        self.unit_mut(who).auras.clear();
    }

    /// Set `who`'s health, as a fraction of its maximum.
    pub fn set_health(&mut self, who: Who, fraction: f32) {
        let c = &mut self.unit_mut(who).combatant;
        c.current_health = c.max_health * fraction;
    }

    /// `who`'s `Combatant`, to change anything the builder does not name.
    pub fn combatant_mut(&mut self, who: Who) -> &mut Combatant {
        &mut self.unit_mut(who).combatant
    }

    /// The auras on `who`, as the scene set them.
    pub fn auras_of(&self, who: Who) -> &[Aura] {
        &self.find(who).auras
    }

    /// The entity standing for `who`.
    pub fn entity(&self, who: Who) -> Entity {
        self.find(who).entity
    }

    fn find(&self, who: Who) -> &SceneUnit {
        self.units
            .iter()
            .find(|u| u.who == who)
            .unwrap_or_else(|| panic!("no unit named {who} in the scene"))
    }

    fn unit_mut(&mut self, who: Who) -> &mut SceneUnit {
        self.units
            .iter_mut()
            .find(|u| u.who == who)
            .unwrap_or_else(|| panic!("no unit named {who} in the scene"))
    }

    /// `u`'s `Combatant` as the decision sees it: its named target resolved.
    fn combatant_of(&self, u: &SceneUnit) -> Combatant {
        let mut combatant = u.combatant.clone();
        if let Some(whom) = u.target {
            combatant.target = Some(self.entity(whom));
        }
        combatant
    }

    fn who(&self, entity_id: u32) -> Who {
        self.units
            .iter()
            .find(|u| u.entity.index() == entity_id)
            .map_or("<not in scene>", |u| u.who)
    }

    /// The snapshot every unit decides from: one row per unit, derived from
    /// its `Combatant` exactly as `CombatSnapshot::build` derives it.
    fn snapshot(&self) -> CombatSnapshot {
        let mut combatants = BTreeMap::new();
        let mut active_auras = BTreeMap::new();
        let mut dr_trackers = BTreeMap::new();
        let mut ability_cooldowns = BTreeMap::new();
        for u in &self.units {
            let transform = Transform::from_translation(u.position);
            let combatant = self.combatant_of(u);
            combatants.insert(
                u.entity,
                CombatantInfo::from_combatant(
                    u.entity, &combatant, &transform, u.casting, None, None,
                ),
            );
            active_auras.insert(u.entity, u.auras.clone());
            dr_trackers.insert(u.entity, u.dr.clone());
            ability_cooldowns.insert(
                u.entity,
                u.combatant
                    .ability_cooldowns
                    .iter()
                    .map(|(k, v)| (*k, *v))
                    .collect(),
            );
        }
        CombatSnapshot {
            combatants,
            active_auras,
            dr_trackers,
            ability_cooldowns,
            obstacles: self.obstacles.clone(),
            bounds: self.bounds,
            ai_profile: self.ai_profile,
        }
    }

    /// Classify both teams' situations from the scene as it stands, with the
    /// previous classification as history — one frame passing — and keep the
    /// result as the history the next `reclassify` or [`Scene::decide`] reads.
    /// A scene that never calls it decides with no history.
    pub fn reclassify(&mut self) -> &Situations {
        let snapshot = self.snapshot();
        let mut trace = DecisionTrace {
            current_sim_time: self.now,
            ..Default::default()
        };
        let clock = MatchClock {
            gates_opened: self.gates_opened,
            ..MatchClock::after_gates(self.now)
        };
        update_situations(
            &mut self.situations,
            &snapshot,
            &clock,
            &SituationConfig::from_movement(&self.movement),
            &mut trace,
        );
        &self.situations
    }

    /// Run `who`'s whole decision for one tick and report it, classifying its
    /// situation with the scene's history ([`Scene::reclassify`]). The scene
    /// is not changed: the decision runs on copies, and its commands are
    /// discarded, so a second `decide` sees the same scene.
    pub fn decide(&self, who: Who) -> DecisionView {
        let unit = self.find(who);
        assert!(
            !unit.auras.iter().any(|a| is_incapacitating(&a.effect_type)),
            "{who} is incapacitated: `decide_abilities` would not dispatch it"
        );
        let snapshot = self.snapshot();
        let mut trace = DecisionTrace {
            current_sim_time: self.now,
            ..Default::default()
        };
        let mut situations = self.situations.clone();
        let clock = MatchClock {
            gates_opened: self.gates_opened,
            ..MatchClock::after_gates(self.now)
        };
        update_situations(
            &mut situations,
            &snapshot,
            &clock,
            &SituationConfig::from_movement(&self.movement),
            &mut trace,
        );
        // The classification's own transitions are not the unit's decision.
        trace.pending_events.clear();
        let ctx = snapshot
            .context_for(unit.entity)
            .with_situation(situations.for_team(unit.combatant.team));

        // The posture state lives in a second world whose entity ids match the
        // first, so the posture query yields exactly the `Mut` items
        // `decide_abilities` hands the dispatch.
        let mut posture_world = World::new();
        for u in &self.units {
            let mut e = posture_world.spawn_empty();
            assert_eq!(e.id(), u.entity, "the two worlds' entity ids agree");
            if u.entity == unit.entity {
                if let Some(p) = u.healer_posture {
                    e.insert(p);
                }
                if let Some(p) = u.kite_posture {
                    e.insert(p);
                }
                if let Some(d) = u.directive {
                    e.insert(d);
                }
                if let Some(r) = u.reset {
                    e.insert(r);
                }
                if let Some(f) = u.flare {
                    e.insert(f);
                }
            }
        }
        let mut posture_query = posture_world.query::<(
            Option<&mut HealerPosture>,
            Option<&mut KitePosture>,
            Option<&MovementDirective>,
            Option<&mut MeleeResetState>,
            Option<&mut RogueFlareState>,
        )>();
        let posture = posture_query.get_mut(&mut posture_world, unit.entity).ok();

        let totems: BTreeMap<Entity, [f32; 4]> = self
            .totems
            .iter()
            .map(|(who, durations)| (self.entity(who), *durations))
            .collect();
        let empty_traps = BTreeMap::new();
        let frame = DispatchFrame {
            abilities: &self.abilities,
            movement_config: &self.movement,
            gates_opened: self.gates_opened,
            now: self.now,
            team1_kill_target: None,
            team2_kill_target: None,
            totem_durations: &totems,
            own_freezing_traps: &empty_traps,
            lit_flares: &[],
        };
        let mut combatant = self.combatant_of(unit);
        let auras = ActiveAuras {
            auras: unit.auras.clone(),
        };
        // The decision's commands run against a third world whose entity ids
        // match the first, and are applied there once it returns — so what it
        // SPAWNED (a trap and where it lands) can be read back. Nothing else
        // reads that world: the scene itself is never changed.
        let mut exec_world = World::new();
        for u in &self.units {
            assert_eq!(exec_world.spawn_empty().id(), u.entity);
        }
        let mut queue = CommandQueue::default();
        let mut commands = Commands::new(&mut queue, &exec_world);
        let mut combat_log = CombatLog::default();
        let mut game_rng = GameRng::from_seed(0);
        let acted = dispatch_class_decision(
            DispatchIo {
                commands: &mut commands,
                combat_log: &mut combat_log,
                game_rng: &mut game_rng,
                decision_trace: &mut trace,
            },
            &frame,
            &mut DispatchAccumulators::default(),
            unit.entity,
            &mut combatant,
            unit.position,
            Some(&auras),
            &ctx,
            posture,
        );
        queue.apply(&mut exec_world);
        let mut view = DecisionView::read(self, unit.entity, acted, &trace);
        let mut placed = exec_world.query::<(&Trap, &Transform)>();
        view.trap_landings.extend(
            placed
                .iter(&exec_world)
                .map(|(trap, at)| (trap.trap_type, at.translation)),
        );
        let mut thrown = exec_world.query::<&TrapLaunchProjectile>();
        view.trap_landings.extend(
            thrown
                .iter(&exec_world)
                .map(|launch| (launch.trap_type, launch.landing_position)),
        );
        view
    }
}

/// One `movement_decision` the unit made, as the trace recorded it.
#[derive(Clone, Debug)]
pub struct MovementView {
    pub posture: Posture,
    pub previous_posture: Option<Posture>,
    pub trigger: MovementTrigger,
    pub chosen_direction: Option<[f32; 2]>,
    pub scorer_terms: BTreeMap<String, f32>,
}

/// What one [`Scene::decide`] did, read off the decision trace: typed, so an
/// assertion names abilities and reasons rather than JSON strings.
#[derive(Clone, Debug)]
pub struct DecisionView {
    /// Whether the dispatch reported that the unit acted.
    pub acted: bool,
    /// The chosen ability and its target's name (`None` for an untargeted
    /// or self-only choice the trace records without a target).
    pub chosen: Option<(AbilityType, Option<Who>)>,
    /// Every ability the decision chose, in order, with its target's name.
    /// One entry per `action_taken` event: a unit that acts more than once
    /// in a tick (an instant, then a cast) has more than one, and
    /// [`Self::chosen`] is the last of them.
    pub choices: Vec<(AbilityType, Option<Who>)>,
    /// Every trap the decision put down or threw, and the point it lands on
    /// (the ground, `y = 0`). A trap's traced target is the enemy it was
    /// AIMED at; this is where it was cast.
    pub trap_landings: Vec<(TrapType, Vec3)>,
    /// Every ability the decision considered, in the order it did.
    pub candidates: Vec<AbilityCandidate>,
    /// The movement decisions it made, in order.
    pub movement: Vec<MovementView>,
    /// The `situation` its `ability_decision` carried.
    pub situation: Option<SituationTag>,
}

impl DecisionView {
    fn read(scene: &Scene, entity: Entity, acted: bool, trace: &DecisionTrace) -> Self {
        let mut view = DecisionView {
            acted,
            chosen: None,
            choices: Vec::new(),
            trap_landings: Vec::new(),
            candidates: Vec::new(),
            movement: Vec::new(),
            situation: None,
        };
        let mine = trace.pending_events.iter().filter(|e| {
            e.actor
                .as_ref()
                .is_some_and(|a| a.entity_id == entity.index())
        });
        for event in mine {
            match (&event.kind, &event.payload) {
                (
                    EventKind::AbilityDecision,
                    EventPayload::Ability {
                        candidates,
                        outcome,
                    },
                ) => {
                    view.candidates.extend(candidates.iter().cloned());
                    view.situation = event.situation;
                    if let AbilityOutcome::ActionTaken {
                        ability, target_id, ..
                    } = outcome
                    {
                        view.chosen = Some((*ability, target_id.map(|id| scene.who(id))));
                        view.choices.extend(view.chosen);
                    }
                }
                (
                    EventKind::MovementDecision,
                    EventPayload::Movement {
                        posture,
                        previous_posture,
                        trigger,
                        chosen_direction,
                        scorer_terms,
                        ..
                    },
                ) => view.movement.push(MovementView {
                    posture: *posture,
                    previous_posture: *previous_posture,
                    trigger: *trigger,
                    chosen_direction: *chosen_direction,
                    scorer_terms: scorer_terms
                        .iter()
                        .flatten()
                        .map(|(k, v)| (k.to_string(), *v))
                        .collect(),
                }),
                _ => {}
            }
        }
        view
    }

    /// The chosen ability and target, as [`Self::chosen`].
    pub fn chose(&self) -> Option<(AbilityType, Option<Who>)> {
        self.chosen
    }

    /// The rejection reason `ability` was traced with, if it was rejected.
    pub fn rejected(&self, ability: AbilityType) -> Option<&RejectionReason> {
        self.candidates
            .iter()
            .find(|c| c.ability == ability && c.status == CandidateStatus::Rejected)
            .and_then(|c| c.reason.as_ref())
    }

    /// `term`'s value in the last movement decision that ran the scorer.
    pub fn scorer_term(&self, term: &str) -> Option<f32> {
        self.movement
            .iter()
            .rev()
            .find(|m| !m.scorer_terms.is_empty())
            .and_then(|m| m.scorer_terms.get(term).copied())
    }
}
