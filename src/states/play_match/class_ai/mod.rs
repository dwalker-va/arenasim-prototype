//! Class-Specific AI Modules
//!
//! This module contains the AI decision logic for each character class.
//! Each class has a standalone `decide_<class>_action()` function that is
//! called from `combat_ai.rs` based on the combatant's `CharacterClass`.
//!
//! ## Architecture
//!
//! The combat AI works in two phases:
//! 1. **Context Building**: `CombatContext` collects all game state needed for decisions
//! 2. **Decision Making**: `combat_ai.rs` dispatches to the appropriate class module's
//!    `decide_<class>_action()` function, which directly executes abilities
//!
//! Shared helpers like `CombatContext`, `CombatantInfo`, and healer utilities
//! live in this module and are used by all class AI files.

pub mod burst;
pub mod cast_guard;
pub mod caster_healer_posture;
pub mod cc_reach;
pub mod chain;
pub mod combat_snapshot;
pub mod dps_postures;
pub mod druid;
pub mod formation;
pub(crate) mod healer_postures;
pub mod hunter;
pub mod hunter_dip;
pub mod hunter_lane;
pub mod interrupts;
pub mod mage;
pub mod paladin;
pub(crate) mod paladin_postures;
pub mod peel;
pub mod pet_ai;
pub mod pillar_mode;
pub mod plays;
pub mod priest;
pub mod rogue;
pub mod rogue_flare;
#[doc(hidden)]
pub mod scene;
pub mod shaman;
pub mod take_cover;
pub mod warlock;
pub mod warrior;

use bevy::prelude::*;
use std::borrow::Cow;
use std::collections::BTreeMap;

use super::abilities::AbilityType;
use super::ability_config::AbilityDefinitions;
use super::ai_profile::AiProfile;
use super::arena_bounds::ArenaBounds;
use super::components::{
    class_base_stats, ActiveAuras, Aura, AuraType, CastingState, Combatant, DRCategory, DRTracker,
    DispelPending, DispelScope, PetType,
};
use super::constants::{GCD, LOW_HP_THRESHOLD, MELEE_RANGE, SAFE_KITING_DISTANCE};
use super::decision_trace::RejectionReason;
use super::is_spell_school_locked;
use super::map_geometry::{has_line_of_sight, ObstacleVolume};
use super::match_config::CharacterClass;
use super::situation::{
    classify_roster, MatchClock, Play, Role, SituationConfig, TeamSituation, Window,
};
use super::utils::log_ability_use;
use crate::combat::log::CombatLog;

/// Per-frame snapshot of a single combatant, used for AI decision making.
#[derive(Clone, Copy, Debug)]
pub struct CombatantInfo {
    pub entity: Entity,
    pub team: u8,
    pub slot: u8,
    pub class: CharacterClass,
    pub current_health: f32,
    pub max_health: f32,
    pub current_mana: f32,
    pub max_mana: f32,
    /// Per-frame snapshot from Transform.
    pub position: Vec3,
    /// Estimated planar velocity (XZ, units/sec): the facing heading (from the
    /// Transform rotation, which `move_to_target` points along travel) scaled by
    /// `base_movement_speed`. `Vec3::ZERO` when the combatant is casting or
    /// channeling (planted) — so a consumer can lead a moving target and drop
    /// directly on a stationary one. Used by the Hunter to lead Freezing Trap
    /// into a kiting target's path. An estimate: a non-casting but idle target
    /// carries a stale heading, but trap targets (healers/casters) are normally
    /// either casting or kiting.
    pub velocity: Vec3,
    pub is_alive: bool,
    pub stealthed: bool,
    pub target: Option<Entity>,
    pub is_pet: bool,
    /// The ability this combatant is currently casting or channeling, if any.
    /// `Some` iff the entity has a live `CastingState`/`ChannelingState` this
    /// frame. Consumers map it to a `SpellSchool` (via `AbilityDefinitions`) to
    /// reason about interruptibility — e.g. the Rogue's Kidney Shot chain firing
    /// on a cast whose school is NOT covered by an active lockout.
    pub casting_ability: Option<AbilityType>,
    pub pet_type: Option<PetType>,
    /// Owner→pet reverse lookup. For pet-owning combatants (Hunter, Warlock)
    /// this is `Some(pet_entity)`. For pets themselves and non-owners, `None`.
    /// Populated by `CombatSnapshot::build` and `pet_ai_system`'s local build.
    pub pet: Option<Entity>,
}

/// Deferred instant melee attack (Mortal Strike, Ambush, Sinister Strike, etc.)
#[derive(Clone, Copy)]
pub struct QueuedInstantAttack {
    pub attacker: Entity,
    pub target: Entity,
    pub damage: f32,
    pub attacker_team: u8,
    pub attacker_slot: u8,
    pub attacker_class: CharacterClass,
    pub ability: AbilityType,
    pub is_crit: bool,
}

/// Deferred AoE damage (Frost Nova).
#[derive(Clone, Copy)]
pub struct QueuedAoeDamage {
    pub caster: Entity,
    pub target: Entity,
    pub damage: f32,
    pub caster_team: u8,
    pub caster_slot: u8,
    pub caster_class: CharacterClass,
    pub target_pos: Vec3,
    pub is_crit: bool,
}

impl CombatantInfo {
    /// This combatant's combat-log id, pet-aware. A pet resolves to its own
    /// display id keyed to its owner's slot (`"Team 1 Spider #2"`), NOT the raw
    /// `combatant_id` (which for a pet would use the owner's class and the
    /// un-adjusted `PET_SLOT_BASE + owner_slot`, yielding an id like
    /// `"Team 1 Hunter #12"` that matches nothing registered). Use this
    /// everywhere a target id is built from a snapshot, so pet targets attribute
    /// correctly and never leak an impossible slot number into log text.
    pub fn log_id(&self) -> crate::combat::log::CombatantId {
        match self.pet_type {
            Some(pt) => super::utils::pet_combatant_id(
                self.team,
                super::utils::owner_relative_slot(self.slot),
                pt,
            ),
            None => super::utils::combatant_id(self.team, self.slot, self.class),
        }
    }

    /// Health as a percentage (0.0 to 1.0)
    pub fn health_pct(&self) -> f32 {
        if self.max_health > 0.0 {
            self.current_health / self.max_health
        } else {
            0.0
        }
    }

    /// Mana as a percentage (0.0 to 1.0)
    pub fn mana_pct(&self) -> f32 {
        if self.max_mana > 0.0 {
            self.current_mana / self.max_mana
        } else {
            0.0
        }
    }

    /// Distance to another position
    pub fn distance_to(&self, other_pos: Vec3) -> f32 {
        self.position.distance(other_pos)
    }
}

/// Shared context for AI decision making.
///
/// This struct provides a read-only view of the game state that AI modules
/// can use to make decisions without directly accessing ECS queries.
///
/// The `combatants` map is what the deciding unit can PERCEIVE: every ally
/// and pet, and every enemy it can see. An enemy it cannot see (a stealthed
/// Rogue, absent Shadow Sight — see [`stealth_visible`]) is simply not in the
/// map, so no class AI, posture scorer or pet can target, peel, kite or
/// trigger on it, whatever it forgets to check. The only way to build a
/// context is [`CombatContext::new`], which applies that filter; the struct
/// has private fields precisely so a literal cannot skip it.
///
/// Use `alive_enemies()` / `alive_allies()` for primary-combatant-only queries.
/// When iterating `combatants` directly, filter with `!info.is_pet`
/// unless the ability should affect pets (e.g., AoE damage, auto-attacks).
pub struct CombatContext<'a> {
    /// The combatants this unit can perceive (see the struct docs), per-frame
    /// snapshot. Borrowed straight from the snapshot whenever nothing is
    /// hidden from this observer, so the filter costs nothing once every
    /// Rogue is revealed.
    /// `BTreeMap` is used (not `HashMap`) so iteration order is deterministic
    /// across runs — required for seeded replays. See `CombatSnapshot` docs.
    pub combatants: Cow<'a, BTreeMap<Entity, CombatantInfo>>,
    /// Every combatant in the arena, hidden ones included. Private: it is
    /// read only by the few questions that are about the arena rather than
    /// about what this unit sees — area-effect victims, team HP totals, and
    /// whether an enemy is unaccounted for.
    roster: &'a BTreeMap<Entity, CombatantInfo>,
    /// Living enemies in `roster` but not in `combatants`.
    hidden_enemies: usize,
    /// Map of entity to their active auras
    pub active_auras: &'a BTreeMap<Entity, Vec<Aura>>,
    /// Map of entity to their DR tracker (for immunity queries)
    pub dr_trackers: &'a BTreeMap<Entity, DRTracker>,
    /// Map of entity to their per-ability cooldowns (per-frame snapshot).
    /// Hunter AI reads this when dispatching pet abilities — it needs to know
    /// the pet's cooldown state without holding a mutable handle to pet
    /// `Combatant`. `BTreeMap` (nested) for determinism.
    pub ability_cooldowns: &'a BTreeMap<Entity, BTreeMap<AbilityType, f32>>,
    /// The active map's obstacle volumes, in declaration order. Empty on maps
    /// with no cover (BasicArena) — where every line-of-sight query is
    /// trivially clear, so the LoS gates are a no-op. Threaded through the
    /// snapshot from `ActiveMapGeometry`; consumed by the cast-start LoS guard
    /// (and, in later units, the movement scorer).
    pub obstacles: &'a [ObstacleVolume],
    /// The active map's walkable region, threaded from `ActiveMapGeometry`
    /// alongside `obstacles`. Consumed wherever the AI places a world point it
    /// expects to be reachable (formation points, trap positions), so those
    /// points respect the selected map's shape rather than a global octagon.
    pub bounds: ArenaBounds,
    /// Which AI implementation this match runs under. Read at the decision sites
    /// of opt-in behaviours; see `ai_profile.rs`.
    pub ai_profile: AiProfile,
    /// The combatant making the decision
    pub self_entity: Entity,
    /// The team whose view this is (a pet decides from a roster it is not in,
    /// so this is not always `self_entity`'s row).
    observer_team: u8,
    /// This unit's team situation — read it through [`Self::situation`].
    /// `None` until classified: see [`Self::situation`].
    situation: Option<TeamSituation>,
}

impl<'a> CombatContext<'a> {
    /// Build the context `self_entity` decides from. `roster` is every
    /// combatant in the arena; the context's `combatants` is the subset
    /// `self_entity` (on `observer_team`) can perceive — every ally, and every
    /// enemy [`stealth_visible`] lets it see. `observer_team` is passed rather
    /// than looked up because a pet decides from a roster it is not in.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        self_entity: Entity,
        observer_team: u8,
        roster: &'a BTreeMap<Entity, CombatantInfo>,
        active_auras: &'a BTreeMap<Entity, Vec<Aura>>,
        dr_trackers: &'a BTreeMap<Entity, DRTracker>,
        ability_cooldowns: &'a BTreeMap<Entity, BTreeMap<AbilityType, f32>>,
        obstacles: &'a [ObstacleVolume],
        bounds: ArenaBounds,
        ai_profile: AiProfile,
    ) -> Self {
        let has_shadow_sight = |entity: Entity| {
            active_auras
                .get(&entity)
                .is_some_and(|auras| auras.iter().any(|a| a.effect_type == AuraType::ShadowSight))
        };
        let observer_has_shadow_sight = has_shadow_sight(self_entity);
        let hidden = |info: &CombatantInfo| {
            info.team != observer_team
                && !stealth_visible(
                    info.stealthed,
                    observer_has_shadow_sight,
                    has_shadow_sight(info.entity),
                )
        };
        let hidden_enemies = roster
            .values()
            .filter(|info| info.is_alive && hidden(info))
            .count();
        let combatants = if roster.values().any(hidden) {
            Cow::Owned(
                roster
                    .iter()
                    .filter(|(_, info)| !hidden(info))
                    .map(|(entity, info)| (*entity, *info))
                    .collect(),
            )
        } else {
            Cow::Borrowed(roster)
        };
        Self {
            combatants,
            roster,
            hidden_enemies,
            active_auras,
            dr_trackers,
            ability_cooldowns,
            obstacles,
            bounds,
            ai_profile,
            self_entity,
            observer_team,
            situation: None,
        }
    }

    /// This context with its team's situation as `decide_abilities` classified
    /// it for the frame (`situation::update_situations`).
    pub fn with_situation(mut self, situation: TeamSituation) -> Self {
        self.situation = Some(situation);
        self
    }

    /// This context with a situation classified from its own roster, at
    /// `clock`, under `cfg`, with no history (every window opens now). For a
    /// test or a standalone caller that builds its context outside
    /// `decide_abilities`: the clock and the thresholds are its to choose,
    /// and choosing them here is what keeps them from being silently wrong.
    pub fn classified(self, cfg: &SituationConfig, clock: MatchClock) -> Self {
        let situation = classify_roster(
            self.roster,
            self.active_auras,
            self.obstacles,
            self.observer_team,
            &TeamSituation::default(),
            &clock,
            cfg,
        );
        self.with_situation(situation)
    }

    /// The situation this unit's team is in (`situation.rs`): the phase, the
    /// tempo, whether the enemy healer is locked and until when, and which
    /// teammates are dying or pressured. Read this rather than re-deriving a
    /// situation at the decision site.
    ///
    /// # Panics
    ///
    /// On a context nobody classified — one built by [`Self::new`] alone,
    /// outside `decide_abilities` (a pet's, a test's). Such a context has no
    /// frame clock and no `movement.ron` thresholds, so any answer would be
    /// silently wrong; classify it with [`Self::classified`] or
    /// [`Self::with_situation`].
    pub fn situation(&self) -> &TeamSituation {
        self.situation.as_ref().expect(
            "situation read from a CombatContext nobody classified: only \
             decide_abilities classifies (with the frame's clock and \
             movement.ron); call `classified` or `with_situation` first",
        )
    }

    /// The play this unit runs as `role`: the pipeline's walk
    /// (`plays::walk`, over [`plays::PLAY_ORDER`]) — the first stage its team
    /// runs and its situation calls ([`TeamSituation::play_for`]) that THIS
    /// unit admits: it has a tool for the play, and the stage's exceptions do
    /// not name it ([`plays::admitted`]). A stage it does not admit passes it
    /// to the next — a peeler its context gates off still chains, and a unit
    /// with nothing to hand off still bursts — and a unit no stage takes runs
    /// [`Play::Default`].
    ///
    /// Panics where [`Self::situation`] does.
    pub fn play(&self, role: Role) -> Play {
        let situation = self.situation();
        plays::walk(situation, role, self.self_entity, None, |stage, play| {
            plays::admitted(self, stage, play)
        })
    }

    /// The lock this unit is handing off from: `Some` when its play is
    /// [`Play::Chain`] ([`TeamSituation::chain_window`]). `None` on a context
    /// nobody classified (a pet's): like [`Self::burst_window`] it does not
    /// panic, because the cast gate asks it of every targeted hold.
    pub fn chain_window(&self) -> Option<Window> {
        let situation = self.situation.as_ref()?;
        let role = Role::of(self.self_info()?.class);
        match self.play(role) {
            Play::Chain { .. } => situation.chain_window(),
            _ => None,
        }
    }

    /// Whether this unit runs the Burst play its team runs: the Burst stage
    /// admits it (its class is not one of the stage's exceptions).
    fn self_bursts(&self) -> bool {
        plays::admitted(self, &burst::BurstStage, &Play::Burst)
    }

    /// The kill window this unit is bursting into: `Some` when its chain runs
    /// under the Burst play — the Burst stage applies to it, whether Burst is
    /// its play or the stage below a Peel or a Chain step that did not fire.
    /// `None` on a context nobody classified (a pet's): unlike
    /// [`Self::situation`] it does not panic, because the cast gate asks it of
    /// every targeted cast ([`cast_guard::fits_window`]).
    pub fn burst_window(&self) -> Option<Window> {
        let situation = self.situation.as_ref()?;
        let role = Role::of(self.self_info()?.class);
        plays::called(&burst::BurstStage, situation, role, self.self_entity)
            .filter(|play| plays::admitted(self, &burst::BurstStage, play))
            .and(situation.kill_window)
    }

    /// The lock-window rule, as `Legacy` has always run it: a DPS whose enemy
    /// healer is locked (`enemy_healer_locked`) reorders its chain toward its
    /// hard hitter (the Warrior's Mortal Strike ahead of Rend, the Hunter's
    /// Aimed Shot ahead of its rotation). The Burst play REPLACES it: a unit
    /// the Burst stage admits ([`plays::admitted`]) never runs this, so it
    /// runs only under `Legacy`, with the play withheld, or for a class the
    /// play's exceptions name. Kept rather than deleted because deleting it
    /// moves `Legacy` (22 of 3,600 matches of the AS-275 byte-identity
    /// batch). Panics where [`Self::situation`] does.
    pub fn lock_window(&self) -> bool {
        let situation = self.situation();
        !self.self_bursts() && situation.enemy_healer_locked.is_some()
    }

    /// Whether this unit's healers play the pillar modes (AS-373,
    /// [`pillar_mode`]): its team runs `TeamPlan` and no measurement turned
    /// them off. `false` on a context nobody classified.
    pub fn pillar_modes(&self) -> bool {
        self.ai_profile.is_team_plan() && self.situation.as_ref().is_some_and(|s| s.pillar_modes)
    }

    /// The healer this unit must keep in sight: `Some` while it is a DPS whose
    /// team's `sight_yield` says it yields to that healer (AS-386,
    /// `situation::SightPair`). `None` on a context nobody classified, and
    /// always under `Legacy`, where the team classifies no yield.
    pub fn sight_yield_healer(&self) -> Option<Entity> {
        self.situation
            .as_ref()?
            .sight_yield_of(self.self_entity)
            .map(|pair| pair.healer)
    }

    /// The clock this unit's situation was classified at (the trace's
    /// `sim_time`); `None` on a context nobody classified.
    pub fn situation_now(&self) -> Option<f32> {
        self.situation.as_ref().map(|s| s.now)
    }

    /// True when a living enemy is in the arena that this unit cannot see —
    /// an opener is unaccounted for. The one perception question that is
    /// answered from the roster: you know the enemy team, not where it is.
    pub fn enemy_hidden(&self) -> bool {
        self.hidden_enemies > 0
    }

    /// Is the enemy a configured kill target of `index` names alive and
    /// hidden from this unit? `index` counts the enemy team's non-pet
    /// combatants in slot order, exactly as target acquisition resolves
    /// `team1_kill_target` / `team2_kill_target`. Answered from the roster
    /// like [`enemy_hidden`](Self::enemy_hidden): a team knows whom it was
    /// told to kill, not where that enemy is.
    pub fn kill_target_hidden(&self, index: usize) -> bool {
        let Some(my_team) = self.self_info().map(|i| i.team) else {
            return false;
        };
        let mut primaries: Vec<&CombatantInfo> = self
            .roster
            .values()
            .filter(|info| info.team != my_team && !info.is_pet)
            .collect();
        primaries.sort_by_key(|info| info.slot);
        primaries
            .get(index)
            .is_some_and(|info| info.is_alive && !self.combatants.contains_key(&info.entity))
    }

    /// Every living enemy of `team` (pets included) physically within
    /// `radius` of `center`, SEEN OR NOT, in deterministic entity order.
    ///
    /// For RESOLVING an area effect, never for deciding to cast one: an area
    /// does not aim, so a stealthed Rogue standing in a Frost Nova is caught by
    /// it (and revealed by its damage — `apply_damage_with_absorb`). Decide
    /// from `combatants`; resolve from here.
    pub fn area_victims(&self, team: u8, center: Vec3, radius: f32) -> Vec<&CombatantInfo> {
        self.roster
            .values()
            .filter(|info| {
                info.team != team && info.is_alive && center.distance(info.position) <= radius
            })
            .collect()
    }

    /// Get info about self
    pub fn self_info(&self) -> Option<&CombatantInfo> {
        self.combatants.get(&self.self_entity)
    }

    /// Get info about target (if any)
    pub fn target_info(&self) -> Option<&CombatantInfo> {
        self.self_info()
            .and_then(|info| info.target)
            .and_then(|target| self.combatants.get(&target))
    }

    /// Get auras on self
    pub fn self_auras(&self) -> Option<&Vec<Aura>> {
        self.active_auras.get(&self.self_entity)
    }

    /// Get auras on target
    pub fn target_auras(&self) -> Option<&Vec<Aura>> {
        self.target_info()
            .and_then(|info| self.active_auras.get(&info.entity))
    }

    /// Check if self has a specific aura type
    pub fn has_aura(&self, aura_type: AuraType) -> bool {
        self.self_auras()
            .map(|auras| auras.iter().any(|a| a.effect_type == aura_type))
            .unwrap_or(false)
    }

    /// Check if target has a specific aura type
    pub fn target_has_aura(&self, aura_type: AuraType) -> bool {
        self.target_auras()
            .map(|auras| auras.iter().any(|a| a.effect_type == aura_type))
            .unwrap_or(false)
    }

    /// Check if self is incapacitated (stunned, feared, or polymorphed).
    /// NOTE: The canonical CC type list lives in `utils::is_incapacitated`.
    /// CombatContext can't delegate because it stores auras as `&[Aura]`, not `&ActiveAuras`.
    pub fn is_incapacitated(&self) -> bool {
        self.has_aura(AuraType::Stun)
            || self.has_aura(AuraType::Fear)
            || self.has_aura(AuraType::Polymorph)
            || self.has_aura(AuraType::Incapacitate)
            || self.has_aura(AuraType::Cyclone)
    }

    /// Check if an entity is currently under hard CC — see [`is_hard_cc`] for
    /// the membership and why it is spelled out there rather than here.
    /// Useful for preventing CC overlap on targets.
    pub fn is_ccd(&self, entity: Entity) -> bool {
        self.active_auras
            .get(&entity)
            .map(|auras| auras.iter().any(|a| is_hard_cc(a.effect_type)))
            .unwrap_or(false)
    }

    /// Is `entity` already HELD — under crowd control that stops it acting
    /// ([`holds`]) for longer than [`CC_CHAIN_WINDOW`]? Returns the holding
    /// aura's type (the longest-lasting one), for the trace.
    ///
    /// The one question every hard-CC chooser asks of the target it would CAST
    /// on, and the cast gate asks again of every targeted cast that would
    /// apply a hold ([`cast_guard::pre_cast_ok`]). A chooser that only moves
    /// the unit (a dip, a trap setup) does not ask: walking to a held healer
    /// puts the caster in place for when the hold ends.
    ///
    /// A fresh hard CC on a held unit spends a cast, a global cooldown and
    /// diminishing returns on a target that is not acting anyway, and can
    /// replace a Polymorph the team placed.
    /// Inside the last [`CC_CHAIN_WINDOW`] the hold is about to end and a new
    /// one CHAINS onto it instead (the Rogue's Kidney Shot onto its own
    /// expiring Cheap Shot).
    ///
    /// `incoming` is how the CC about to be cast takes damage. One that
    /// [`Incoming::Lasts`] (a stun, a horror, a Cyclone) is not stacked on a
    /// hold the team is breaking: a hold that breaks once enough damage
    /// accumulates (a Fear, a Psychic Scream) does not count while the team is
    /// attacking its bearer, so the stun becomes the lock that lasts. A hold
    /// that breaks on ANY damage (a Polymorph, a Freezing Trap) still counts —
    /// the team holds its damage for it ([`Self::has_friendly_breakable_cc`]).
    /// One that [`Incoming::Breaks`] itself would only be broken the same way,
    /// so every hold counts against it.
    ///
    /// Asked of enemies, whose crowd control can only be the deciding unit's
    /// own team's: no hold lands on its caster's side. So the caster is not
    /// looked up — a hold whose caster has died or left the view holds all the
    /// same.
    pub fn held_in_hard_cc(&self, entity: Entity, incoming: Incoming) -> Option<AuraType> {
        self.held_in_hard_cc_at(entity, incoming, 0.0)
    }

    /// [`Self::held_in_hard_cc`], asked of the moment a cast begun now LANDS,
    /// `lands_after` seconds from now: a hold counts only if more than
    /// [`CC_CHAIN_WINDOW`] of it will be left then. The Chain play's hand-off
    /// asks it this way ([`cast_guard::hold_lands_after`]), so a 1.5s cast
    /// started to land in a lock's last moments is not refused at its start
    /// for the lock it will chain onto. With `lands_after` 0 it is
    /// [`Self::held_in_hard_cc`] exactly.
    pub fn held_in_hard_cc_at(
        &self,
        entity: Entity,
        incoming: Incoming,
        lands_after: f32,
    ) -> Option<AuraType> {
        let breaking = incoming == Incoming::Lasts && self.attacked_by_my_team(entity);
        self.active_auras
            .get(&entity)?
            .iter()
            .filter(|a| holds(a.effect_type) && a.duration - lands_after > CC_CHAIN_WINDOW)
            .filter(|a| !(breaking && a.break_on_damage_threshold > 0.0))
            .max_by(|a, b| a.duration.total_cmp(&b.duration))
            .map(|a| a.effect_type)
    }

    /// Does a living member of the deciding unit's team (itself and pets
    /// included) have `entity` as its target?
    pub fn attacked_by_my_team(&self, entity: Entity) -> bool {
        let my_team = self.self_info().map(|i| i.team);
        self.combatants
            .values()
            .any(|c| Some(c.team) == my_team && c.is_alive && c.target == Some(entity))
    }

    /// The enemy healer — first alive non-pet healer in deterministic entity
    /// order (BTreeMap), if any. The Hunter's freezing-trap targeting and the
    /// Priest's Mana Burn read it; whether it is LOCKED is a situation
    /// ([`Self::situation`]), classified from the same healer.
    pub fn enemy_healer(&self) -> Option<Entity> {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.combatants
            .iter()
            .find(|(_, info)| {
                info.team != my_team && info.is_alive && !info.is_pet && info.class.is_healer()
            })
            .map(|(entity, _)| *entity)
    }

    /// Get all alive enemies (excluding pets)
    pub fn alive_enemies(&self) -> Vec<&CombatantInfo> {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.combatants
            .values()
            .filter(|c| c.team != my_team && c.is_alive && !c.is_pet)
            .collect()
    }

    /// Get all alive allies (including self, excluding pets)
    pub fn alive_allies(&self) -> Vec<&CombatantInfo> {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.combatants
            .values()
            .filter(|c| c.team == my_team && c.is_alive && !c.is_pet)
            .collect()
    }

    /// Get the lowest health ally a heal can reach — never a cycloned one.
    pub fn lowest_health_ally(&self) -> Option<&CombatantInfo> {
        self.alive_allies()
            .into_iter()
            .filter(|info| !self.is_cycloned(info.entity))
            .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap())
    }

    /// Find the lowest-health ally below a given HP percentage threshold, within
    /// range, excluding pets — and excluding a cycloned ally, which no heal
    /// reaches ([`Self::is_cycloned`]). The healers' heal-target pick, so a
    /// healer heals the next ally instead of a Cyclone.
    pub fn lowest_health_ally_below(
        &self,
        max_hp_pct: f32,
        max_range: f32,
        my_pos: Vec3,
    ) -> Option<&CombatantInfo> {
        self.alive_allies()
            .into_iter()
            .filter(|info| {
                !info.is_pet
                    && !self.is_cycloned(info.entity)
                    && info.health_pct() < max_hp_pct
                    && my_pos.distance(info.position) <= max_range
            })
            .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap())
    }

    /// [`Self::lowest_health_ally_below`], choosing only among allies the cast
    /// REACHES ([`cast_reach`]: range, then sight). An occluded lowest ally
    /// yields to the lowest one in sight, rather than being picked and then
    /// refused. With nobody in reach the error is the reason to trace:
    /// `LosBlocked` when an ally qualified on range and only sight stood in the
    /// way, `NoValidTarget` otherwise. With every candidate in sight (any
    /// obstacle-free map) it picks exactly what `lowest_health_ally_below` does.
    pub fn lowest_health_ally_in_reach(
        &self,
        max_hp_pct: f32,
        max_range: f32,
        my_pos: Vec3,
    ) -> Result<&CombatantInfo, RejectionReason> {
        let mut occluded = false;
        self.alive_allies()
            .into_iter()
            .filter(|info| {
                !info.is_pet && !self.is_cycloned(info.entity) && info.health_pct() < max_hp_pct
            })
            .filter(
                |info| match cast_reach(self, max_range, my_pos, info.position) {
                    CastReach::Reaches => true,
                    CastReach::OutOfRange { .. } => false,
                    CastReach::LosBlocked => {
                        occluded = true;
                        false
                    }
                },
            )
            .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap())
            .ok_or(if occluded {
                RejectionReason::LosBlocked
            } else {
                RejectionReason::NoValidTarget
            })
    }

    /// Returns true if all allies are above the given HP threshold.
    pub fn is_team_healthy(&self, threshold: f32, my_pos: Vec3) -> bool {
        self.lowest_health_ally_below(threshold, f32::MAX, my_pos)
            .is_none()
    }

    /// Team-HP-fraction advantage of the deciding combatant's team — the
    /// press-when-ahead signal. Own team's summed alive-member health fraction
    /// minus the sum of every other team's: positive = ahead, negative = behind,
    /// `0.0` = level (or self missing from the snapshot). Pets excluded and dead
    /// members contribute 0, mirroring [`is_team_healthy`]'s conventions.
    ///
    /// Deterministic: the per-team sums accumulate in BTreeMap (entity) order
    /// via [`team_hp_sums`], so seeded replays agree to the bit and the two
    /// teams' advantages are exact negations of one another (same summands,
    /// opposite sign). Cheap enough to call per decision — the snapshot holds a
    /// handful of combatants — so it needs no cached field on the context.
    pub fn team_hp_advantage(&self) -> f32 {
        let Some(my_team) = self.self_info().map(|i| i.team) else {
            return 0.0;
        };
        // The roster, not the view: a hidden Rogue's health is still on the
        // enemy team's side of the ledger.
        let sums = team_hp_sums(self.roster);
        let own = sums.get(&my_team).copied().unwrap_or(0.0);
        let enemy: f32 = sums
            .iter()
            .filter(|(team, _)| **team != my_team)
            .map(|(_, sum)| *sum)
            .sum();
        own - enemy
    }

    // ------------------------------------------------------------------
    // Threat predicates (healer postures — R6/R7 trigger and window inputs)
    // ------------------------------------------------------------------

    /// Visible enemies whose current target is `me`. Enemy pets count as
    /// threats (`is_pet` entities are included, unlike `alive_enemies()`).
    /// Stealth-filtered by construction (`combatants` holds only what `me`
    /// can see), so healers never pre-dodge invisible Rogues. `me` must be
    /// the context's own `self_entity` — the view is that unit's perception.
    ///
    /// Iterates the `BTreeMap` snapshot, so the returned order is
    /// deterministic (ascending `Entity`).
    pub fn enemies_targeting(&self, me: Entity) -> Vec<&CombatantInfo> {
        let Some(my_team) = self.combatants.get(&me).map(|i| i.team) else {
            return Vec::new();
        };
        self.combatants
            .values()
            .filter(|c| c.team != my_team && c.is_alive && c.target == Some(me))
            .collect()
    }

    /// Visible alive enemies (pets included) within `radius` of `pos` —
    /// the proximity half of the PRESSURED threat set (an enemy in your face
    /// is a threat even when it currently targets someone else). Same
    /// perception view as `enemies_targeting`; same deterministic BTree order.
    pub fn visible_enemies_within(
        &self,
        me: Entity,
        pos: Vec3,
        radius: f32,
    ) -> Vec<&CombatantInfo> {
        let Some(my_team) = self.combatants.get(&me).map(|i| i.team) else {
            return Vec::new();
        };
        self.combatants
            .values()
            .filter(|c| c.team != my_team && c.is_alive && pos.distance(c.position) <= radius)
            .collect()
    }

    /// The nearest alive visible enemy (including pets) currently targeting
    /// `me`. Ties resolve to the lowest `Entity` (BTreeMap iteration order)
    /// for determinism.
    pub fn primary_attacker(&self, me: Entity) -> Option<&CombatantInfo> {
        let my_pos = self.combatants.get(&me)?.position;
        self.enemies_targeting(me).into_iter().min_by(|a, b| {
            my_pos
                .distance(a.position)
                .partial_cmp(&my_pos.distance(b.position))
                .unwrap()
        })
    }

    /// Remaining movement-impairment window on `attacker`: the longest
    /// remaining Root/Stun/Incapacitate duration, or `None` if the attacker
    /// is free to move. Fear is deliberately excluded — a feared attacker
    /// wanders away on its own, so it is not a reliable escape window.
    pub fn attacker_escape_window(&self, attacker: Entity) -> Option<f32> {
        self.active_auras.get(&attacker).and_then(|auras| {
            auras
                .iter()
                .filter(|a| {
                    matches!(
                        a.effect_type,
                        AuraType::Root
                            | AuraType::Stun
                            | AuraType::Incapacitate
                            | AuraType::Cyclone
                    )
                })
                .map(|a| a.duration)
                .max_by(|a, b| a.partial_cmp(b).unwrap())
        })
    }

    /// Product of `MovementSpeedSlow` magnitudes currently on `entity`
    /// (`1.0` = unslowed; `0.5` = moving at half speed). Mirrors the
    /// executor's slow handling in `move_to_target`, so the ESCAPE window
    /// math (R7) predicts the same effective speed the directive will
    /// actually move at.
    pub fn movement_slow_multiplier(&self, entity: Entity) -> f32 {
        self.active_auras
            .get(&entity)
            .map(|auras| {
                auras
                    .iter()
                    .filter(|a| a.effect_type == AuraType::MovementSpeedSlow)
                    .map(|a| a.magnitude)
                    .product()
            })
            .unwrap_or(1.0)
    }

    /// Derived closing intent — no velocity history (keeps the hot path free
    /// of mutable state). `threat` is closing on `me` when its kill target is
    /// `me` AND its pursuit movement would reduce the distance this frame,
    /// i.e. it currently sits beyond its preferred range to me (pursuit in
    /// `move_to_target` walks toward targets outside `preferred_range` and
    /// holds position inside it). A stationary caster already in range
    /// targeting me is NOT closing.
    pub fn is_closing(&self, threat: Entity, me: Entity) -> bool {
        let Some(threat_info) = self.combatants.get(&threat) else {
            return false;
        };
        let Some(my_info) = self.combatants.get(&me) else {
            return false;
        };
        if !threat_info.is_alive || threat_info.target != Some(me) {
            return false;
        }
        let preferred = match threat_info.pet_type {
            Some(pet_type) => pet_type.preferred_range(),
            None => threat_info.class.preferred_range(),
        };
        threat_info.distance_to(my_info.position) > preferred
    }

    /// Check if target has a break-on-any-damage CC from a friendly caster.
    /// Uses threshold-based detection: any aura with `break_on_damage_threshold == 0.0`
    /// (breaks on ANY damage) from a same-team caster is protected.
    /// Used to prevent AI from breaking own team's CC with damage/DoTs.
    pub fn has_friendly_breakable_cc(&self, target: Entity) -> bool {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.active_auras
            .get(&target)
            .map(|auras| {
                auras.iter().any(|a| {
                    a.break_on_damage_threshold == 0.0
                        && a.caster
                            .and_then(|c| self.combatants.get(&c).map(|info| info.team))
                            == Some(my_team)
                })
            })
            .unwrap_or(false)
    }

    /// Check if target has DoTs from a friendly caster that would break Polymorph/Freezing Trap.
    pub fn has_friendly_dots_on_target(&self, target: Entity) -> bool {
        let my_team = self.self_info().map(|i| i.team).unwrap_or(0);
        self.active_auras
            .get(&target)
            .map(|auras| {
                auras.iter().any(|a| {
                    a.effect_type == AuraType::DamageOverTime
                        && a.caster
                            .and_then(|c| self.combatants.get(&c).map(|info| info.team))
                            == Some(my_team)
                })
            })
            .unwrap_or(false)
    }

    /// Check if an entity is immune to damage — Divine Shield or a Cyclone
    /// ([`grants_damage_immunity`], the one predicate target acquisition and
    /// the interrupt checks also ask). An attacker never spends a cast on it.
    pub fn entity_is_immune(&self, entity: Entity) -> bool {
        self.active_auras
            .get(&entity)
            .is_some_and(|auras| auras.iter().any(|a| grants_damage_immunity(a.effect_type)))
    }

    /// Whether `entity` is CYCLONED: out of reach of every spell, friendly ones
    /// included. A cycloned ally takes no healing, no buff and no dispel
    /// (`ActiveAuras::is_cycloned`, which the engine's funnels ask), so a healer
    /// never spends a cast on one.
    pub fn is_cycloned(&self, entity: Entity) -> bool {
        self.active_auras
            .get(&entity)
            .is_some_and(|auras| auras.iter().any(|a| a.effect_type == AuraType::Cyclone))
    }

    /// Check if an entity is DR-immune to a specific CC category.
    /// AI uses this to avoid wasting CC abilities into immunity.
    pub fn is_dr_immune(&self, entity: Entity, category: DRCategory) -> bool {
        self.dr_trackers
            .get(&entity)
            .map(|tracker| tracker.is_immune(category))
            .unwrap_or(false)
    }

    /// Start a decision-trace `ability_decision` builder for the current
    /// actor. Returns None only when the snapshot doesn't contain self
    /// (defensive — shouldn't happen in normal dispatch).
    ///
    /// Replaces the actor_view + target_view + builder boilerplate that
    /// every `decide_<class>_action` had to assemble by hand.
    pub fn start_ability_decision<'t>(
        &self,
        decision_trace: &'t mut crate::states::play_match::decision_trace::DecisionTrace,
        target: Option<Entity>,
        my_pos: Vec3,
    ) -> Option<crate::states::play_match::decision_trace::DecisionEventBuilder<'t>> {
        use crate::states::play_match::decision_trace::{ActorView, TargetView};
        let actor_view_class = self.self_info()?.class;
        let actor_view = ActorView::from_info(self.self_info()?);
        let target_view = target
            .and_then(|t| self.combatants.get(&t))
            .map(|info| TargetView::from_info(info, my_pos));
        Some(match &self.situation {
            Some(situation) => decision_trace
                .start_ability_decision(actor_view, target_view)
                .with_situation(situation)
                .with_play(self.play(Role::of(actor_view_class))),
            None => decision_trace.start_ability_decision(actor_view, target_view),
        })
    }
}

// ============================================================================
// Shared Team-State Utilities
// ============================================================================

/// Summed health fraction of each team's alive, non-pet members, keyed by team
/// id. The press-when-ahead advantage signal derives from these sums;
/// [`CombatContext::team_hp_advantage`] is own-team minus the rest. Deterministic
/// — accumulates in BTreeMap (entity) order. Mirrors
/// [`CombatContext::is_team_healthy`]'s alive/`!is_pet` conventions; a dead
/// member contributes 0 (excluded), so a wiped team sums to `0.0`.
pub fn team_hp_sums(combatants: &BTreeMap<Entity, CombatantInfo>) -> BTreeMap<u8, f32> {
    let mut sums: BTreeMap<u8, f32> = BTreeMap::new();
    for info in combatants.values() {
        if info.is_alive && !info.is_pet {
            *sums.entry(info.team).or_insert(0.0) += info.health_pct();
        }
    }
    sums
}

/// THE stealth visibility rule — the one copy. An enemy is visible unless it
/// is stealthed, and a stealthed enemy is visible anyway when the observer
/// holds Shadow Sight or the enemy does (picking up the buff reveals the
/// holder). Target acquisition (`combat_ai::acquire_targets`) and every class
/// AI's view ([`CombatContext::new`]) both ask here.
pub fn stealth_visible(
    enemy_stealthed: bool,
    observer_has_shadow_sight: bool,
    enemy_has_shadow_sight: bool,
) -> bool {
    !enemy_stealthed || observer_has_shadow_sight || enemy_has_shadow_sight
}

/// Press-when-ahead predicate: own team leads by at least the margin. The test
/// behind the team situation's `tempo` (`situation.rs`), which the healer deny
/// postures and the Warrior tempo reset read (both "stop the defensive behavior
/// when clearly ahead"). The tempo latch applies it twice, at the onset
/// (`press_advantage_margin`) and, for a team already pressing or trailing, at
/// the release (`press_release_margin`, `TeamPlan` only). Pure for unit
/// testing.
pub(crate) fn pressing_when_ahead(advantage: f32, margin: f32) -> bool {
    advantage >= margin
}

// ============================================================================
// Shared Targeting Utilities
// ============================================================================

/// Bucket A target-swap chooser (pure). Given the kill target's current HP and
/// an iterator of eligible melee candidates `(entity, distance, current_health)`,
/// returns the SOFTEST (lowest current HP) candidate within `swap_range` whose
/// current HP is at least `hp_margin` (a fraction of the kill target's CURRENT
/// HP) below it — so a swap is only offered when it meaningfully shortens
/// time-to-kill, never for a trivial difference. Deterministic tie-break by
/// entity. Returns `None` when nothing qualifies.
///
/// The caller is responsible for passing only ELIGIBLE candidates (alive,
/// non-pet, visible, not immune) and excluding the current kill target itself.
/// Kept context-free so it unit-tests in isolation and composes with the raw
/// tuple lists in `acquire_targets`.
pub fn select_softer_melee_target<I>(
    kill_target_health: f32,
    candidates: I,
    swap_range: f32,
    hp_margin: f32,
) -> Option<Entity>
where
    I: IntoIterator<Item = (Entity, f32, f32)>,
{
    let threshold = kill_target_health * (1.0 - hp_margin);
    candidates
        .into_iter()
        .filter(|(_, distance, _)| *distance <= swap_range)
        .filter(|(_, _, health)| *health <= threshold)
        .min_by(|(ea, _, ha), (eb, _, hb)| ha.partial_cmp(hb).unwrap().then(ea.cmp(eb)))
        .map(|(entity, _, _)| entity)
}

// ============================================================================
// Shared Healer Utilities
// ============================================================================

/// Calculate dispel priority for an aura type.
/// Higher values = more urgent to dispel.
/// Used by Priest (Dispel Magic) and Paladin (Cleanse).
///
/// The number is only ever compared against a caller's `min_priority`, and the
/// game has exactly two bars: **90** (urgent — dispel ahead of healing, even
/// under pressure) and **50** (maintenance — team-healthy only). A `0` here
/// therefore does not mean "low", it means "no AI will ever remove this".
///
/// Priority picks the ALLY to dispel, not the aura: the aura actually removed
/// is a random draw among that ally's dispellable auras (`process_dispels`,
/// intentionally). Raising a type buys a dispel CAST aimed at its holder, not a
/// guaranteed removal — and it spends that GCD, so a promotion here is always
/// also a demotion of whatever the healer would otherwise have lifted.
///
/// **Exhaustive on purpose — do not add a `_ =>` arm.** A wildcard is exactly
/// how `Incapacitate` (Freezing Trap) and `Silence` (the Unstable Affliction
/// backlash) sat at 0 for the life of the project: both are
/// [`AuraType::is_magic_dispellable`], the removal path worked end to end, and
/// across 32 seeded matches carrying 19 dispels and 46 cleanses neither was
/// ever removed — not because anyone decided they should not be, but because
/// they fell through the wildcard. The compiler refusing to build until variant
/// N+1 is graded is the only guard that holds.
pub fn dispel_priority(aura_type: AuraType) -> i32 {
    match aura_type {
        // ---- Urgent band: clears BOTH caller bars (90 and 50). ----
        // Complete incapacitates. Polymorph (10s) and Freezing Trap (8s) share
        // `DRCategory::Incapacitates`, both break on ANY damage, and both remove
        // the holder from the match outright — there is no honest basis for
        // ranking one above the other, so they tie.
        AuraType::Polymorph | AuraType::Incapacitate => 100,
        // Unstable Affliction's backlash silence (5s). Above Fear because it is
        // the only crowd control in the game that does NOT break on damage
        // (`break_on_damage_threshold: -1.0`), so a dispel is the sole way it
        // ends early, where a feared/sheeped/trapped ally is freed by incidental
        // cleave. Below the incapacitates because it leaves movement and
        // auto-attacks intact — the holder is disabled, not helpless.
        AuraType::Silence => 95,
        // Loss of control; breaks at 100 cumulative damage.
        AuraType::Fear => 90,

        // ---- Maintenance band: only the team-healthy caller (50) reaches these. ----
        AuraType::Root => 80,           // Can't move
        AuraType::DamageOverTime => 50, // Taking damage

        // ---- Below every caller's bar: deliberately never dispelled. ----
        // A snare is not worth a GCD that could be a heal, and the caster
        // re-applies it on the next cast anyway.
        AuraType::MovementSpeedSlow => 20,

        // ---- Graded 0 deliberately: not dispel candidates at all. ----
        // Undispellable debuffs. `is_magic_dispellable` already rejects these,
        // so they never reach this function; grading them keeps "make it
        // dispellable" a two-place decision instead of a silent promotion.
        // Cyclone is the one crowd control here that is undispellable by RULE
        // (TBC) rather than by removal class — see `is_magic_dispellable`.
        AuraType::Stun
        | AuraType::Cyclone
        | AuraType::SpellSchoolLockout
        | AuraType::HealingReduction
        | AuraType::AttackPowerReduction
        | AuraType::AttackSpeedSlow
        | AuraType::DamageReduction
        | AuraType::CastTimeIncrease => 0,

        // Mechanical markers, not effects. Lifting `WeakenedSoul` would hand the
        // Priest a free Power Word: Shield reset.
        AuraType::WeakenedSoul | AuraType::ShadowSight | AuraType::WeaponPoison => 0,

        // Beneficial auras. This function grades DEBUFFS on allies; stripping an
        // enemy's buffs is [`purge_priority`]'s job.
        AuraType::Absorb
        | AuraType::MaxHealthIncrease
        | AuraType::MaxManaIncrease
        | AuraType::AttackPowerIncrease
        | AuraType::SpellPowerIncrease
        | AuraType::HealingOverTime
        | AuraType::WindfuryBuff
        | AuraType::DamageTakenReduction
        | AuraType::DamageImmunity
        | AuraType::CritChanceIncrease
        | AuraType::ManaRegenIncrease
        | AuraType::LockoutDurationReduction
        | AuraType::FrostArmorBuff
        | AuraType::SpellResistanceBuff
        | AuraType::ArmorIncrease
        | AuraType::FearImmunity
        | AuraType::TravelForm => 0,
    }
}

/// The reason to purge: a target is worth a Purge only if it carries a buff
/// worth at least this ([`purge_pick_value`]) — high-value defensives and
/// HoT-class sustain — so Purge never spends a GCD on a target holding only
/// cheap re-buffs like Fortitude. Whether the random pick is then worth
/// taking is [`PurgeOdds::favourable`].
pub const PURGE_MIN_PRIORITY: i32 = 70;

/// The urgent purge bar: a buff worth stripping ahead of the purger's filler
/// damage, not just in the GCDs it has nothing better for. Above every type's
/// default, so only a per-aura override reaches it — today Innervate alone.
pub const PURGE_URGENT_PRIORITY: i32 = 110;

/// What a purge pick that lands on a BLOOMING aura costs, on the
/// [`purge_pick_value`] scale. The pick takes one stack, and that stack blooms
/// ([`Aura::bloom_heal`]): a heal handed to the enemy.
///
/// Set at [`PURGE_MIN_PRIORITY`] — one stack's bloom given away costs as much
/// as the least buff worth a purge gains. So a coin flip between Innervate and
/// a Lifebloom is worth a cast, and one between a Rejuvenation and a Lifebloom
/// is not.
pub const PURGE_BLOOM_COST: i32 = PURGE_MIN_PRIORITY;

/// What one purge pick that lands on this aura INSTANCE is worth. Keyed by the
/// aura, not by its type, because two auras of one type can be worth opposite
/// things:
///
/// - A BLOOMING aura (Lifebloom, [`Aura::bloom_heal`]) is worth
///   **-[`PURGE_BLOOM_COST`]**: the stack a purge takes blooms, which HEALS its
///   bearer.
/// - An aura whose source ability sets `purge_priority` in its RON
///   `applies_aura` is worth that ([`AbilityDefinitions::source_effect`]):
///   Innervate is [`PURGE_URGENT_PRIORITY`], where its type,
///   `ManaRegenIncrease`, is Mage Armor's 25.
/// - Everything else is worth its type ([`purge_priority`]). Rejuvenation is
///   a `HealingOverTime` at 70, exactly the floor.
///
/// The purger never chooses which buff comes off — `process_dispels` picks
/// one at random — so these values are weighed together by [`PurgeOdds`],
/// never used to pick. Every AI that strips an ENEMY's buff asks this. Today
/// that is the Shaman's Purge alone: Devour Magic and Master's Call free allies
/// ([`ally_removal`]).
pub fn purge_pick_value(aura: &Aura, abilities: &AbilityDefinitions) -> i32 {
    if aura.bloom.is_some() {
        return -PURGE_BLOOM_COST;
    }
    abilities
        .source_effect(aura)
        .and_then(|effect| effect.purge_priority)
        .unwrap_or_else(|| purge_priority(aura.effect_type))
}

/// The odds of one Purge on one target. `process_dispels` takes ONE of the
/// target's purgeable buffs ([`DispelScope::Purge`]), uniformly at random over
/// the AURAS — a three-stack Lifebloom is one candidate, as likely as a
/// one-stack Rejuvenation — so the purge is worth the mean of every
/// candidate's [`purge_pick_value`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PurgeOdds {
    /// Buffs the purge could take, each an equal chance.
    pub candidates: usize,
    /// How many of them bloom when taken.
    pub blooming: usize,
    /// The most valuable of them: the reason to purge at all, held to the
    /// caller's bar ([`PURGE_MIN_PRIORITY`] or [`PURGE_URGENT_PRIORITY`]).
    pub best: i32,
    /// The expected value of the random pick.
    pub expected: f32,
}

impl PurgeOdds {
    /// The odds over `auras` (one target's), or `None` when a purge would
    /// take nothing.
    pub fn of<'a>(
        auras: impl IntoIterator<Item = &'a Aura>,
        abilities: &AbilityDefinitions,
    ) -> Option<Self> {
        let mut candidates = 0usize;
        let mut blooming = 0usize;
        let mut best = i32::MIN;
        let mut total = 0i32;
        for aura in auras.into_iter().filter(|a| DispelScope::Purge.takes(a)) {
            let value = purge_pick_value(aura, abilities);
            candidates += 1;
            blooming += usize::from(aura.bloom.is_some());
            best = best.max(value);
            total += value;
        }
        (candidates > 0).then(|| Self {
            candidates,
            blooming,
            best,
            expected: total as f32 / candidates as f32,
        })
    }

    /// Whether the random pick is worth the cast: its expected value is
    /// positive. A Lifebloom stack drags the odds down by its share of the
    /// candidates, so the purge stays worth it beside Innervate and stops
    /// being worth it as blooms crowd out what is worth taking.
    pub fn favourable(&self) -> bool {
        self.expected > 0.0
    }

    /// The trace note for a purge held on these odds.
    pub fn held_note(&self) -> String {
        format!(
            "purge odds: expected {:.1} over {} buffs ({} blooming)",
            self.expected, self.candidates, self.blooming
        )
    }
}

/// A purge the purger's AI has chosen to cast: on whom, and the odds. Never
/// WHICH buff — that is the random pick's.
#[derive(Clone, Debug, PartialEq)]
pub struct PurgeChoice {
    pub target: Entity,
    pub position: Vec3,
    pub odds: PurgeOdds,
}

/// The purge worth casting: among living, non-pet enemies the purge REACHES
/// ([`cast_reach`]: within `range`, then in sight) whose [`PurgeOdds`] carry a
/// buff worth at least `min_priority` AND are [`PurgeOdds::favourable`], the
/// enemy with the best expected pick. Reach filters BEFORE value ranks, so an
/// occluded enemy with the best odds yields to the best one in sight instead
/// of refusing the whole cast. Ties prefer the enemy HEALER (deny its
/// defensives first), then the lowest entity (BTreeMap order —
/// deterministic).
///
/// With no purge in reach the error is the reason to trace: `LosBlocked` when
/// an enemy in range was worth the purge and only sight stood in the way;
/// `PreconditionUnmet`, its note carrying the odds ([`PurgeOdds::held_note`]),
/// when an enemy carried a buff worth the purge but the pick was not worth
/// the gamble; `NoValidTarget` otherwise.
pub fn select_purge(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    my_team: u8,
    my_pos: Vec3,
    range: f32,
    min_priority: i32,
) -> Result<PurgeChoice, RejectionReason> {
    let enemy_healer = ctx.enemy_healer();
    // (choice, is_healer)
    let mut best: Option<(PurgeChoice, bool)> = None;
    let mut occluded = false;
    let mut held: Option<PurgeOdds> = None;
    for (e, info) in ctx.combatants.iter() {
        if info.team == my_team || !info.is_alive || info.is_pet {
            continue;
        }
        // Range gate; sight is asked below, once the enemy is worth a purge
        // (pre_cast_ok re-checks both on the winner, with mana).
        if my_pos.distance(info.position) > range {
            continue;
        }
        let Some(enemy_auras) = ctx.active_auras.get(e) else {
            continue;
        };
        let Some(odds) = PurgeOdds::of(enemy_auras.iter(), abilities) else {
            continue;
        };
        if odds.best < min_priority {
            continue;
        }
        if !odds.favourable() {
            // The first held enemy names the odds in the trace.
            held.get_or_insert(odds);
            continue;
        }
        match cast_reach(ctx, range, my_pos, info.position) {
            CastReach::Reaches => {}
            CastReach::OutOfRange { .. } => continue,
            CastReach::LosBlocked => {
                occluded = true;
                continue;
            }
        }
        let is_healer = enemy_healer == Some(*e);
        let better = match &best {
            None => true,
            Some((b, best_heal)) => {
                odds.expected > b.odds.expected
                    || (odds.expected == b.odds.expected && is_healer && !best_heal)
            }
        };
        if better {
            best = Some((
                PurgeChoice {
                    target: *e,
                    position: info.position,
                    odds,
                },
                is_healer,
            ));
        }
    }
    match (best, held) {
        (Some((choice, _)), _) => Ok(choice),
        (None, _) if occluded => Err(RejectionReason::LosBlocked),
        (None, Some(odds)) => Err(RejectionReason::PreconditionUnmet {
            note: odds.held_note(),
        }),
        (None, None) => Err(RejectionReason::NoValidTarget),
    }
}

/// Calculate purge priority for a BENEFICIAL aura on an enemy.
/// Higher values = more valuable to strip with Purge.
///
/// Defensive buffs (shields/absorbs and incoming-damage reductions) outrank
/// offensive buffs (attack/spell power, crit), which outrank minor utility
/// buffs (mana regen, lockout reduction, resistances). Mirrors
/// [`dispel_priority`] but for the offensive (enemy-buff-strip) direction.
/// Only auras for which [`Aura::can_be_purged`] is true should be passed here.
///
/// **Exhaustive on purpose — do not add a `_ =>` arm.** Same hazard class as
/// [`dispel_priority`] above: a purgeable buff that fell through a wildcard
/// would score 0, land below [`PURGE_MIN_PRIORITY`], and never be purged by any
/// AI — while reading as fully wired, because the aura WOULD be
/// [`Aura::can_be_purged`] and the removal path WOULD exist. Nobody would
/// notice. Make the compiler force the grading instead.
pub fn purge_priority(aura_type: AuraType) -> i32 {
    match aura_type {
        // Defensives — most valuable to remove (denies mitigation / sustain).
        AuraType::Absorb => 100,              // PW:Shield / damage absorb
        AuraType::DamageTakenReduction => 90, // flat damage cut (Devotion Aura is untyped)
        AuraType::HealingOverTime => 70,      // ongoing sustain (Rejuvenation)
        // Offensive throughput buffs.
        AuraType::AttackPowerIncrease => 60,
        AuraType::SpellPowerIncrease => 60,
        AuraType::WindfuryBuff => 55,
        AuraType::CritChanceIncrease => 50,
        // Minor utility buffs.
        AuraType::MaxManaIncrease => 30,
        AuraType::ManaRegenIncrease => 25,
        AuraType::SpellResistanceBuff => 20,
        AuraType::ArmorIncrease => 20,
        AuraType::FrostArmorBuff => 20,
        AuraType::MaxHealthIncrease => 15, // cheap re-buff (PW:Fortitude) — not worth a GCD to strip
        AuraType::LockoutDurationReduction => 15,

        // ---- Graded 0 deliberately: not purge candidates. ----
        // Beneficial but unpurgeable by design (see [`Aura::can_be_purged`]):
        // Divine Shield and Berserker Rage's fear immunity are cooldowns, not
        // buffs to strip; Travel Form is a shapeshift, not a buff at all.
        AuraType::DamageImmunity | AuraType::FearImmunity | AuraType::TravelForm => 0,

        // Mechanical markers, not buffs.
        AuraType::ShadowSight | AuraType::WeaponPoison | AuraType::WeakenedSoul => 0,

        // Debuffs and crowd control — the enemy's problem, not something to
        // remove FOR them. Lifting an ally's debuff is [`dispel_priority`]'s job.
        AuraType::MovementSpeedSlow
        | AuraType::Root
        | AuraType::Stun
        | AuraType::Fear
        | AuraType::Polymorph
        | AuraType::Incapacitate
        | AuraType::Cyclone
        | AuraType::Silence
        | AuraType::SpellSchoolLockout
        | AuraType::DamageOverTime
        | AuraType::HealingReduction
        | AuraType::DamageReduction
        | AuraType::CastTimeIncrease
        | AuraType::AttackPowerReduction
        | AuraType::AttackSpeedSlow => 0,
    }
}

/// Is this aura type HARD crowd control — an effect the AI treats as "this
/// target is already locked down, do not spend another CC on it"?
///
/// Written as an exhaustive `match` with NO wildcard, and that is the whole
/// point of the function existing at all. It used to be a `matches!` inline in
/// [`CombatContext::is_ccd`], and a `matches!` has an implicit `false` arm: a
/// new `AuraType` that IS hard CC reads as not-CC everywhere `is_ccd` gates —
/// burst windows, cast guards, healer-CC checks — with nothing anywhere
/// failing to say so. The doc comment on `is_ccd` had already drifted (it
/// listed four types for a list of five) without a single test noticing.
///
/// Spelled out, adding a variant to `AuraType` cannot build until someone
/// decides which side of this line it falls on. Same shape, and for the same
/// reason, as [`dispel_priority`] below — whose own comment records what a
/// wildcard cost the last time: `Incapacitate` and `Silence` sat ungraded for
/// the life of the project because they fell through one.
pub const fn is_hard_cc(aura: AuraType) -> bool {
    match aura {
        // Acting is prevented outright, or movement is (a rooted target is
        // still a target you should not also sheep).
        AuraType::Stun
        | AuraType::Fear
        | AuraType::Root
        | AuraType::Polymorph
        | AuraType::Incapacitate
        | AuraType::Cyclone => true,

        // Everything else: impairments, buffs, debuffs, damage and healing
        // effects. None of them stop a target acting, so none of them make a
        // fresh CC redundant.
        AuraType::MovementSpeedSlow
        | AuraType::MaxHealthIncrease
        | AuraType::DamageOverTime
        | AuraType::SpellSchoolLockout
        | AuraType::HealingReduction
        | AuraType::MaxManaIncrease
        | AuraType::AttackPowerIncrease
        | AuraType::ShadowSight
        | AuraType::Absorb
        | AuraType::WeakenedSoul
        | AuraType::DamageReduction
        | AuraType::CastTimeIncrease
        | AuraType::DamageTakenReduction
        | AuraType::DamageImmunity
        | AuraType::SpellResistanceBuff
        | AuraType::ArmorIncrease
        | AuraType::AttackPowerReduction
        | AuraType::CritChanceIncrease
        | AuraType::ManaRegenIncrease
        | AuraType::AttackSpeedSlow
        | AuraType::LockoutDurationReduction
        | AuraType::FrostArmorBuff
        | AuraType::Silence
        | AuraType::WeaponPoison
        | AuraType::SpellPowerIncrease
        | AuraType::HealingOverTime
        | AuraType::WindfuryBuff
        | AuraType::FearImmunity
        | AuraType::TravelForm => false,
    }
}

/// Does this aura HOLD its bearer — stop it acting, so that a second hard
/// crowd control on it would be wasted? [`is_hard_cc`] less Root: a rooted
/// unit still casts and swings, so a root makes no stun, fear, polymorph or
/// cyclone redundant. Derived from the exhaustive [`is_hard_cc`], so a new
/// hard-CC aura joins it without anyone remembering to.
///
/// The one copy of the set: [`CombatContext::held_in_hard_cc`] and the cast
/// gate ask it of a target, and `situation::locks_healer` (the
/// `enemy_healer_locked` window) and `utils::is_incapacitating` (the engine's
/// "cannot act" test) are it.
pub const fn holds(aura: AuraType) -> bool {
    is_hard_cc(aura) && !matches!(aura, AuraType::Root)
}

/// How a hard CC about to be cast takes damage — what
/// [`CombatContext::held_in_hard_cc`] needs to know of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Incoming {
    /// It breaks on damage (a Polymorph, a Fear, a Freezing Trap).
    Breaks,
    /// It never does (a stun, a horror, a Cyclone).
    Lasts,
}

impl Incoming {
    /// Read off the ability's own aura: a negative `break_on_damage` never
    /// breaks.
    pub fn of(def: &crate::states::play_match::ability_config::AbilityConfig) -> Self {
        match def.applies_aura.as_ref() {
            Some(a) if a.break_on_damage < 0.0 => Incoming::Lasts,
            _ => Incoming::Breaks,
        }
    }
}

/// Seconds before a hold ([`holds`]) ends inside which a fresh hard CC may be
/// cast onto it: it then CHAINS onto the hold rather than stacking on it
/// ([`CombatContext::held_in_hard_cc`]). The Rogue's Kidney Shot chain onto
/// its own Cheap Shot fires at exactly this point.
pub const CC_CHAIN_WINDOW: f32 = 0.5;

/// Does this aura make its holder IMMUNE TO DAMAGE — a target no attacker,
/// interrupt or Mana Burn should spend a cast on?
///
/// Divine Shield, and a Cyclone (TBC: a cycloned unit takes no damage and no
/// healing). The AI's one immunity predicate: [`CombatContext::entity_is_immune`],
/// target acquisition and the interrupt checks all ask here, so they cannot
/// drift apart the way they once did (Divine Shield was immune everywhere, a
/// Cyclone nowhere).
///
/// Exhaustive on purpose, for the reason [`is_hard_cc`] gives.
pub const fn grants_damage_immunity(aura: AuraType) -> bool {
    match aura {
        AuraType::DamageImmunity | AuraType::Cyclone => true,

        AuraType::MovementSpeedSlow
        | AuraType::Root
        | AuraType::Stun
        | AuraType::Fear
        | AuraType::Polymorph
        | AuraType::Incapacitate
        | AuraType::MaxHealthIncrease
        | AuraType::DamageOverTime
        | AuraType::SpellSchoolLockout
        | AuraType::HealingReduction
        | AuraType::MaxManaIncrease
        | AuraType::AttackPowerIncrease
        | AuraType::ShadowSight
        | AuraType::Absorb
        | AuraType::WeakenedSoul
        | AuraType::DamageReduction
        | AuraType::CastTimeIncrease
        | AuraType::DamageTakenReduction
        | AuraType::SpellResistanceBuff
        | AuraType::ArmorIncrease
        | AuraType::AttackPowerReduction
        | AuraType::CritChanceIncrease
        | AuraType::ManaRegenIncrease
        | AuraType::AttackSpeedSlow
        | AuraType::LockoutDurationReduction
        | AuraType::FrostArmorBuff
        | AuraType::Silence
        | AuraType::WeaponPoison
        | AuraType::SpellPowerIncrease
        | AuraType::HealingOverTime
        | AuraType::WindfuryBuff
        | AuraType::FearImmunity
        | AuraType::TravelForm => false,
    }
}

/// Whether an interrupter should take this cast ahead of every other: an enemy
/// Druid's CYCLONE, which removes a teammate from the fight for its duration.
/// The Druid's heals are all instant, so its crowd control is its only
/// interruptible cast — and the lockout lands on Nature, the school of every
/// heal it has. Asked by every interrupter (Kick, Pummel and Wind Shear in
/// `check_interrupts`, Spell Lock in the Felhunter's AI) through
/// [`priority_interrupt_target`].
pub fn is_priority_interrupt(ability: AbilityType) -> bool {
    matches!(ability, AbilityType::Cyclone)
}

/// Whether an interrupt can take this cast: the ability can be interrupted
/// ([`AbilityConfig::can_be_interrupted`](super::ability_config::AbilityConfig::can_be_interrupted)
/// — a cast bar, and Silence-prevented) and the cast is not already
/// interrupted. An instant routed through a zero-length `CastingState` (the
/// Druid's heals, Frost Shock) is no target, and neither is an uninterruptible
/// cast (Aimed Shot): every interrupter's candidate scan (`check_interrupts`
/// for Kick, Pummel and Wind Shear, the Felhunter's Spell Lock) asks here, so
/// none spends its interrupt on one, and `process_interrupts` asks again at
/// resolution, where an interrupt that finds one does nothing — no cut, no
/// lockout.
pub fn cast_is_interruptible(cast: &CastingState, abilities: &AbilityDefinitions) -> bool {
    !cast.interrupted && abilities.get_unchecked(&cast.ability).can_be_interrupted()
}

/// Why an interrupter passes over a cast it can see: the cast is
/// [uninterruptible](super::ability_config::AbilityConfig::is_uninterruptible_cast)
/// (Aimed Shot). `None` for every other cast — an interruptible one, an
/// instant routed through a zero-length cast (nothing to trace: no cast bar
/// was ever up), or one already interrupted. Each interrupter traces a `Some`
/// as [`RejectionReason::Uninterruptible`].
pub fn uninterruptible_cast(
    cast: &CastingState,
    abilities: &AbilityDefinitions,
) -> Option<AbilityType> {
    let def = abilities.get_unchecked(&cast.ability);
    (!cast.interrupted && def.is_uninterruptible_cast()).then_some(cast.ability)
}

/// An enemy mid-cast that an interrupter can reach: in its interrupt's range,
/// cast interruptible ([`cast_is_interruptible`]), and not immune.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InterruptCandidate {
    pub entity: Entity,
    /// The ability being cast.
    pub ability: AbilityType,
    pub distance: f32,
}

/// The enemy an interrupter must interrupt FIRST, whatever its usual pick: the
/// nearest candidate casting an [`is_priority_interrupt`] cast (entity order
/// breaking ties), or `None` when nobody is — and the interrupter's own
/// ranking applies unchanged.
pub fn priority_interrupt_target<I>(candidates: I) -> Option<Entity>
where
    I: IntoIterator<Item = InterruptCandidate>,
{
    candidates
        .into_iter()
        .filter(|c| is_priority_interrupt(c.ability))
        .min_by(|a, b| {
            a.distance
                .total_cmp(&b.distance)
                .then(a.entity.cmp(&b.entity))
        })
        .map(|c| c.entity)
}

/// How far a dispel ABILITY reaches across the removal classes. Every dispel
/// takes MAGIC (that is what `Aura::can_be_dispelled` answers); this says
/// whether it additionally takes POISON and DISEASE.
///
/// The full per-class truth, which `dispel_reach_matches_the_removal_model`
/// pins against the predicates rather than restating here:
/// - Priest's Dispel Magic — magic only.
/// - Paladin's Cleanse — magic + poison/disease.
/// - Felhunter's Devour Magic — magic only.
/// - Nothing reaches CURSE or PHYSICAL. A curse is removable in principle and
///   has no ability serving it yet; a physical debuff yields only to effects
///   that clear physical harm outright, which are not dispels at all and do not
///   come through here (Divine Shield's retain, Master's Call's typed filter).
///
/// Named rather than inlined so that truth is testable. It used to be an `==`
/// buried mid-function, which is a fine way to write a rule and a poor way to
/// let anyone check it.
pub fn dispel_removes_poison(ability_type: AbilityType) -> bool {
    matches!(ability_type, AbilityType::PaladinCleanse)
}

/// The [`DispelScope`] an ally dispel ability removes under — the reach
/// [`dispel_removes_poison`] describes, as the value `process_dispels` and the
/// AI's own candidate scan both ask.
pub fn ally_dispel_scope(ability_type: AbilityType) -> DispelScope {
    if dispel_removes_poison(ability_type) {
        DispelScope::MagicOrPoison
    } else {
        DispelScope::Magic
    }
}

/// The movement impairments Master's Call lifts — roots and slows, whatever
/// their removal class. Shared by the cast (`pet_ai`) and [`ally_removal`].
pub const MASTERS_CALL_IMPAIRMENTS: [AuraType; 2] = [AuraType::Root, AuraType::MovementSpeedSlow];

/// How an ability frees a TEAMMATE: the removal it performs, and whether its
/// candidate scan reaches the team's pets.
#[derive(Clone, Debug, PartialEq)]
pub struct AllyRemoval {
    pub scope: DispelScope,
    pub reaches_pets: bool,
}

/// The ally removal `ability` performs, or `None` when it frees nobody.
///
/// Every `is_dispel` ability is answered here BY NAME —
/// `ally_removal_names_every_dispel` fails until a new one is classified, so a
/// dispel cannot join the game without deciding whether it frees allies. The
/// `reaches_pets` column is the pet rule the casting AIs apply: the healers'
/// shared scan ([`try_dispel_ally`]) skips pets, Devour Magic's
/// (`pet_ai::try_devour_magic`) takes any living teammate. Both scans read
/// their scope and pet reach from here, so change a dispel here and its cast
/// and every question asked of it (the Hunter's "can anyone free my trap's
/// victim?") move together. Master's Call shares only its scope
/// ([`MASTERS_CALL_IMPAIRMENTS`]): it frees roots and slows, never a trap.
pub fn ally_removal(ability: AbilityType) -> Option<AllyRemoval> {
    match ability {
        AbilityType::DispelMagic | AbilityType::PaladinCleanse => Some(AllyRemoval {
            scope: ally_dispel_scope(ability),
            reaches_pets: false,
        }),
        AbilityType::DevourMagic => Some(AllyRemoval {
            scope: ally_dispel_scope(ability),
            reaches_pets: true,
        }),
        AbilityType::MastersCall => Some(AllyRemoval {
            scope: DispelScope::Impairments(MASTERS_CALL_IMPAIRMENTS.to_vec()),
            reaches_pets: false,
        }),
        // Strips an ENEMY's buff; frees nobody.
        AbilityType::Purge => None,
        _ => None,
    }
}

/// Could `freer` lift `aura` off its teammate `victim`? Asked of the freer's own
/// kit (a pet's by its pet type, anyone else's by class) through
/// [`ally_removal`] and the removal scope `process_dispels` applies — the
/// engine's rules, not a list of classes. A unit never frees itself: whatever
/// `aura` is, the question is whether a THIRD party can.
pub fn can_free_ally(
    abilities: &AbilityDefinitions,
    freer: &CombatantInfo,
    victim: &CombatantInfo,
    aura: &Aura,
) -> bool {
    if freer.entity == victim.entity || freer.team != victim.team || !freer.is_alive {
        return false;
    }
    ally_removals_of(abilities, freer)
        .any(|r| (r.reaches_pets || !victim.is_pet) && r.scope.takes(aura))
}

/// Could `freer` lift `aura` off a (non-pet) teammate at all — is it a
/// dispeller for this aura, whoever its teammates turn out to be? The question
/// to ask when the teammate may be one the asker cannot see: a stealthed Rogue
/// is not in the Hunter's view, but its Priest still frees it.
pub fn frees_teammates(abilities: &AbilityDefinitions, freer: &CombatantInfo, aura: &Aura) -> bool {
    freer.is_alive && ally_removals_of(abilities, freer).any(|r| r.scope.takes(aura))
}

/// The ally removals in `unit`'s own kit: a pet's by its pet type, anyone
/// else's by class.
fn ally_removals_of<'a>(
    abilities: &'a AbilityDefinitions,
    unit: &'a CombatantInfo,
) -> impl Iterator<Item = AllyRemoval> + 'a {
    abilities
        .iter()
        .filter(move |(_, def)| match unit.pet_type {
            Some(pet) => def.pet == Some(pet),
            None => def.class == unit.class && def.pet.is_none(),
        })
        .filter_map(|(ability, _)| ally_removal(*ability))
}

/// How far `unit` can interrupt a cast from: the longest range among the
/// `is_interrupt` abilities in its own kit (a pet's by its pet type, anyone
/// else's by class), or `None` when it has no interrupt. Read from the config,
/// so a new interrupt is counted without a code change.
pub fn interrupt_reach(abilities: &AbilityDefinitions, unit: &CombatantInfo) -> Option<f32> {
    abilities
        .iter()
        .filter(|(_, def)| match unit.pet_type {
            Some(pet) => def.pet == Some(pet),
            None => def.class == unit.class && def.pet.is_none(),
        })
        .filter(|(_, def)| def.is_interrupt)
        .map(|(_, def)| def.range)
        .reduce(f32::max)
}

/// Every living teammate of `victim` in this view that could free it from
/// `aura` ([`can_free_ally`]), in deterministic entity order.
pub fn ally_freers(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    victim: Entity,
    aura: &Aura,
) -> Vec<Entity> {
    let Some(victim_info) = ctx.combatants.get(&victim) else {
        return Vec::new();
    };
    ctx.combatants
        .values()
        .filter(|freer| can_free_ally(abilities, freer, victim_info, aura))
        .map(|freer| freer.entity)
        .collect()
}

/// The urgent dispel bar: crowd control worth a healer's GCD ahead of healing,
/// even under pressure (see [`dispel_priority`]).
pub const URGENT_DISPEL_PRIORITY: i32 = 90;

/// How urgently a healer's ally dispel wants `aura` gone: [`dispel_priority`],
/// except that a cleansable poison (e.g. Crippling's 70% slow) is worth a
/// maintenance cleanse — rated 50 rather than the bare MovementSpeedSlow's 20,
/// so a healthy Paladin lifts it, but an under-pressure Paladin (urgent-only)
/// still prioritizes healing over the snare.
pub fn ally_dispel_priority(aura: &Aura) -> i32 {
    if aura.is_cleansable_poison() {
        50
    } else {
        dispel_priority(aura.effect_type)
    }
}

/// The root bar: a magic root that is HOLDING A TEAMMATE BACK
/// ([`root_holds_back`]) is worth spending a dispel ahead of routine healing.
/// The dispel is spent BECAUSE of the root; what it removes is still the random
/// pick among everything its scope takes (`process_dispels`), as for every
/// ally dispel. The bar sits below the
/// urgent crowd control ([`URGENT_DISPEL_PRIORITY`]: a rooted teammate still
/// casts, swings and heals, a sheeped one does nothing) and above the routine
/// magic dispels (a root that is not holding anyone back, a DoT — the bare
/// [`dispel_priority`]). A dispel walk carries the healer to one only when the
/// walk is short and lands with root still to remove ([`owed_dispel`]): a root
/// lasts 4-8s and breaks at 80 damage, so a long walk would arrive as it ended,
/// having taken the healer off its post to get there.
pub const ROOT_DISPEL_PRIORITY: i32 = 85;

/// Whether the root on `ally` is keeping it from doing its job — the situation
/// that makes a root worth a dispel's GCD ahead of a heal:
/// - a MELEE teammate ([`CharacterClass::is_melee`]) whose target has stepped
///   out of its swing range ([`MELEE_RANGE`]) — Frost Nova's whole purpose, the
///   Mage walking off the Rogue it froze; or
/// - a RANGED teammate being trained: a visible enemy chaser (a melee class or
///   a pet) targeting it stands inside [`SAFE_KITING_DISTANCE`], and the root
///   keeps it from stepping out.
///
/// A rooted melee teammate still swinging at its target, or a rooted caster
/// nobody is on, is not held back: the root costs it nothing yet, so it waits
/// for the routine dispels. Reads only the caller's perceived view, so an
/// unseen chaser does not count.
///
/// Never the healer itself (`ctx.self_entity`): a root on the healer stays a
/// routine dispel. A rooted healer still casts, and spending its GCD on its own
/// root while a teammate needs a heal trades the team's healing for the
/// healer's footing. The dispel walk skips the healer for the same reason.
pub fn root_holds_back(ctx: &CombatContext, ally: &CombatantInfo) -> bool {
    if ally.entity == ctx.self_entity {
        return false;
    }
    if ally.class.is_melee() && !ally.is_pet {
        return ally
            .target
            .and_then(|t| ctx.combatants.get(&t))
            .is_some_and(|t| t.is_alive && ally.position.distance(t.position) > MELEE_RANGE);
    }
    ctx.combatants.values().any(|enemy| {
        enemy.team != ally.team
            && enemy.is_alive
            && enemy.target == Some(ally.entity)
            && (enemy.is_pet || enemy.class.is_melee())
            && enemy.position.distance(ally.position) <= SAFE_KITING_DISTANCE
    })
}

/// How urgently a healer's ally dispel wants `aura` gone from `ally` in this
/// situation: [`ally_dispel_priority`], except that a root holding the
/// teammate back ([`root_holds_back`]) rises to [`ROOT_DISPEL_PRIORITY`].
pub fn situational_ally_dispel_priority(
    ctx: &CombatContext,
    ally: &CombatantInfo,
    aura: &Aura,
) -> i32 {
    if aura.effect_type == AuraType::Root && root_holds_back(ctx, ally) {
        ROOT_DISPEL_PRIORITY
    } else {
        ally_dispel_priority(aura)
    }
}

/// Whether a targeted cast of range `range` from `from` reaches a unit at `to`.
///
/// The two gates every targeted cast passes at cast start (`pre_cast_ok`), in
/// the same order — range first, then line of sight, so an out-of-range unit is
/// reported as out of range, never as occluded. Every path that CHOOSES its
/// target from several candidates asks this one question while choosing, so an
/// occluded best pick yields to the best one in reach instead of refusing the
/// cast: the healers' dispels ([`try_dispel_ally`]), Devour Magic, Master's
/// Call, Purge ([`select_purge`]), every heal that ranks allies by health
/// ([`CombatContext::lowest_health_ally_in_reach`]) — and the Psychic Scream gate
/// that holds for one of them — Holy Shock damage, Hammer of Justice (the
/// rotation's pick and the dip's arrival), the Frost Shock peel, every
/// interrupt's caster scan (Kick, Pummel, Wind Shear and Spell Lock,
/// `class_ai::interrupts`), and — through [`sight_blocks`] — the choosers
/// that rank without a range
/// filter (Power Word: Shield and Fortitude, Mark of the Wild, the Druid's heal
/// focus and its Cyclone and Entangling Roots peels); and the walk that carries
/// a healer to a teammate it cannot yet free ([`dispel_chase_target`]) — so the
/// walk ends exactly where the cast becomes possible. On an obstacle-free map
/// sight always holds and this is the range check alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CastReach {
    Reaches,
    OutOfRange { distance: f32 },
    LosBlocked,
}

pub fn cast_reach(ctx: &CombatContext, range: f32, from: Vec3, to: Vec3) -> CastReach {
    let distance = from.distance(to);
    if distance > range {
        CastReach::OutOfRange { distance }
    } else if !has_line_of_sight(ctx.obstacles, from, to) {
        CastReach::LosBlocked
    } else {
        CastReach::Reaches
    }
}

/// Whether SIGHT alone keeps a cast of `range` from `from` off a unit at `to`:
/// in range, but occluded ([`cast_reach`] says `LosBlocked`).
///
/// For the choosers that rank their candidates without a range filter — Power
/// Word: Shield, Power Word: Fortitude, Mark of the Wild, the Druid's heal
/// focus, its Cyclone and Entangling Roots peel picks, and its step-8 Cyclone's
/// enemy healer. They drop such a
/// candidate before ranking, so an occluded best pick yields to the best one
/// in sight, while a best pick beyond range is still
/// picked and refused exactly as it always was. On an obstacle-free map this is
/// never true and the choice is unchanged.
pub fn sight_blocks(ctx: &CombatContext, range: f32, from: Vec3, to: Vec3) -> bool {
    cast_reach(ctx, range, from, to) == CastReach::LosBlocked
}

/// Whether `entity` could cast `dispel` this instant as far as its own state
/// goes: not silenced, and not locked out of the dispel's school. A healer that
/// cannot cast its dispel has no reason to walk for it — the walk would carry it
/// into reach, release there, and hand it back to a posture that steps it out
/// again, strobing at the range edge for the whole lockout.
pub fn can_cast_dispel(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    entity: Entity,
    dispel: AbilityType,
) -> bool {
    let Some(def) = abilities.get(&dispel) else {
        return false;
    };
    let auras = ctx.active_auras.get(&entity);
    let uses_mana = ctx
        .combatants
        .get(&entity)
        .is_some_and(|c| c.class.uses_mana());
    if def.silence_stops() && super::abilities::silence_holds(uses_mana, auras.map(Vec::as_slice)) {
        return false;
    }
    // `is_spell_school_locked` takes `ActiveAuras`; the context stores the aura
    // vec directly, so rebuild the thin wrapper rather than duplicate the
    // school decoding it owns.
    let wrapped = auras.map(|a| ActiveAuras { auras: a.clone() });
    !is_spell_school_locked(def.spell_school, wrapped.as_ref())
}

/// The teammate a healer should WALK to so that `dispel` can free it: the
/// nearest living non-pet teammate (never the healer itself) holding crowd
/// control that `dispel` removes at the urgent bar — or, for the Paladin's
/// Cleanse, a root holding it back within a short walk ([`owed_dispel`]) —
/// which `dispel` does not
/// reach from where the healer stands — beyond its range, or in range but out of
/// sight ([`cast_reach`]). `None` when such a teammate is already reached (the
/// rotation's urgent dispel frees it where the healer stands), when there is
/// none, or when the healer cannot afford the dispel on arrival.
///
/// A walk toward a teammate in range but behind cover is the same `Point` walk:
/// tangent steering rounds the pillar, and the walk ends the moment the
/// teammate is both in range and in sight.
///
/// The urgent bar is the same one the rotation's urgent dispel uses, so the
/// walk is only ever toward a dispel the rotation would cast the moment it
/// arrives. Nearest first, entity order breaking ties, so it is deterministic.
pub fn dispel_chase_target(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    entity: Entity,
    my_pos: Vec3,
    current_mana: f32,
    dispel: AbilityType,
) -> Option<Entity> {
    match owed_dispel(ctx, abilities, entity, my_pos, current_mana, dispel)? {
        OwedDispel::Unreached(ally) => Some(ally),
        OwedDispel::Reached(_) => None,
    }
}

/// An urgent dispel a healer owes a teammate — see [`owed_dispel`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwedDispel {
    /// The dispel reaches this teammate from where the healer stands: the
    /// rotation's urgent dispel frees it on its next GCD.
    Reached(Entity),
    /// No owed teammate is reached; this is the nearest one, which the healer
    /// must walk to ([`dispel_chase_target`]).
    Unreached(Entity),
}

/// Whether the healer owes `dispel` to a teammate held in crowd control it
/// removes at the urgent bar, and whether the dispel reaches one from `my_pos`.
/// A reached teammate wins over every unreached one (the rotation frees it
/// first, where the healer stands); otherwise the nearest unreached one, entity
/// order breaking ties. `None` when no teammate holds such crowd control, or the
/// healer cannot afford the dispel.
///
/// Only when nothing is owed at the urgent bar, a dispel whose rotation clears
/// roots at the root bar ([`clears_held_roots`] — the Paladin's Cleanse) also
/// owes a ROOT holding a teammate back ([`root_holds_back`]), under two limits
/// that keep the walk cheap and correct:
/// - the rotation's own gate for the root bar holds ([`root_dispel_allowed`]),
///   so the walk is only toward a Cleanse the rotation casts on arrival;
/// - an unreached root is owed only when the healer gets into range within
///   [`ROOT_DISPEL_WALK_MAX_SECS`] at its class's base speed AND with a GCD of
///   the root still left on arrival. A root lasts 4-8s and breaks at 80 damage;
///   a longer walk takes the healer off its post to arrive as it ends.
pub fn owed_dispel(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    entity: Entity,
    my_pos: Vec3,
    current_mana: f32,
    dispel: AbilityType,
) -> Option<OwedDispel> {
    let def = abilities.get(&dispel)?;
    let AllyRemoval { scope, .. } = ally_removal(dispel)?;
    if current_mana < def.mana_cost {
        return None;
    }
    let urgent = |ally: &CombatantInfo| -> bool {
        ctx.active_auras.get(&ally.entity).is_some_and(|auras| {
            auras
                .iter()
                .any(|a| scope.takes(a) && ally_dispel_priority(a) >= URGENT_DISPEL_PRIORITY)
        })
    };
    if let Some(owed) = owed_by(ctx, def.range, entity, my_pos, urgent) {
        return Some(owed);
    }
    if !clears_held_roots(dispel) || !root_dispel_allowed(ctx) {
        return None;
    }
    let speed = ctx
        .combatants
        .get(&entity)
        .map_or(0.0, |me| class_base_stats(me.class).movement_speed);
    let held_root = |ally: &CombatantInfo| -> bool {
        if !root_holds_back(ctx, ally) {
            return false;
        }
        // Seconds to walk into range; zero when already there.
        let arrival = if speed > 0.0 {
            (my_pos.distance(ally.position) - def.range).max(0.0) / speed
        } else {
            f32::INFINITY
        };
        arrival <= ROOT_DISPEL_WALK_MAX_SECS
            && ctx.active_auras.get(&ally.entity).is_some_and(|auras| {
                auras.iter().any(|a| {
                    a.effect_type == AuraType::Root && scope.takes(a) && a.duration >= arrival + GCD
                })
            })
    };
    owed_by(ctx, def.range, entity, my_pos, held_root)
}

/// [`owed_dispel`]'s scan over the healer's living non-pet teammates that
/// `owes` picks out: the first one the dispel reaches, else the nearest
/// unreached one, entity order breaking ties.
fn owed_by(
    ctx: &CombatContext,
    range: f32,
    entity: Entity,
    my_pos: Vec3,
    owes: impl Fn(&CombatantInfo) -> bool,
) -> Option<OwedDispel> {
    let mut nearest: Option<(f32, Entity)> = None;
    for ally in ctx.alive_allies() {
        // Never itself, and never a cycloned teammate: no dispel reaches one.
        if ally.entity == entity || ctx.is_cycloned(ally.entity) {
            continue;
        }
        if !owes(ally) {
            continue;
        }
        if cast_reach(ctx, range, my_pos, ally.position) == CastReach::Reaches {
            return Some(OwedDispel::Reached(ally.entity));
        }
        let distance = my_pos.distance(ally.position);
        if nearest.is_none_or(|(d, e)| (distance, ally.entity) < (d, e)) {
            nearest = Some((distance, ally.entity));
        }
    }
    nearest.map(|(_, e)| OwedDispel::Unreached(e))
}

/// The dispels whose rotation clears a root holding a teammate back at
/// [`ROOT_DISPEL_PRIORITY`] — so the only ones a walk may carry to one. The
/// Paladin's Cleanse; the Priest's rotation has no root tier.
pub fn clears_held_roots(dispel: AbilityType) -> bool {
    dispel == AbilityType::PaladinCleanse
}

/// The gate on the root bar, shared by the rotation and the walk: no living
/// non-pet teammate (the healer and the rooted one included) is below
/// [`LOW_HP_THRESHOLD`]. A rooted teammate still casts and swings; a dying one
/// is the higher-value GCD.
pub fn root_dispel_allowed(ctx: &CombatContext) -> bool {
    !ctx.alive_allies()
        .iter()
        .any(|a| a.health_pct() < LOW_HP_THRESHOLD)
}

/// The longest walk, in seconds at the healer's class base speed, that a root
/// holding a teammate back is worth ([`owed_dispel`]).
pub const ROOT_DISPEL_WALK_MAX_SECS: f32 = 2.0;

/// The dying-first rule for a dispel owed to `ally`: whether some OTHER
/// teammate than the healer and `ally` is dying in the team's situation
/// ([`TeamSituation::dying_allies`]: living, non-pet, below
/// `urgency_hp_threshold`). While one is, no dispel walk runs — the heal is the
/// higher-value GCD (a teammate in CC loses its actions for the CC's length, a
/// dying one for the match), and a walk to the CC'd teammate can carry the
/// healer off the dying one's heal range and sight. The CC'd teammate itself
/// does not count: walking to it brings it into heal range too.
pub fn another_teammate_dying(ctx: &CombatContext, entity: Entity, ally: Entity) -> bool {
    ctx.situation()
        .dying_allies()
        .any(|a| a != entity && a != ally)
}

/// What a healer's ally-dispel candidate scan found ([`scan_ally_dispel`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AllyDispelScan {
    /// The ally to dispel: the reachable one whose best removable debuff is
    /// worth the most, first in entity order on a tie.
    pub target: Option<Entity>,
    /// The nearest ally that WOULD qualify but stands beyond the dispel's
    /// range — reported when nobody reachable qualifies, so a trace tells
    /// "nothing to dispel" apart from "something to dispel, out of reach".
    pub nearest_out_of_range: Option<f32>,
    /// Some qualifying ally stands in range but out of sight. Reported ahead of
    /// a distant one: `LosBlocked` names the nearer miss.
    pub los_blocked: bool,
}

/// The healers' ally-dispel candidate scan (Priest's Dispel Magic, Paladin's
/// Cleanse): among living teammates `dispel` may free ([`ally_removal`] — scope
/// and pet reach), the one whose highest-priority debuff the dispel TAKES
/// ([`DispelScope::takes`]) is worth at least `min_priority`, and which the
/// dispel reaches ([`cast_reach`]).
///
/// Two exclusions are structural, never a low score:
/// - a debuff the scope does not take is not a candidate at all — a Cyclone is
///   never dispellable (`AuraType::is_magic_dispellable`), so no bar, however
///   low, makes it one;
/// - a CYCLONED teammate is not a candidate ([`CombatContext::is_cycloned`]):
///   nothing reaches it, so a debuff underneath the Cyclone waits for it to end.
pub fn scan_ally_dispel(
    ctx: &CombatContext,
    dispel: AbilityType,
    my_team: u8,
    my_pos: Vec3,
    range: f32,
    min_priority: i32,
) -> AllyDispelScan {
    let Some(AllyRemoval {
        scope,
        reaches_pets,
    }) = ally_removal(dispel)
    else {
        return AllyDispelScan::default();
    };
    let mut scan = AllyDispelScan::default();
    let mut best: Option<(Entity, i32)> = None;

    for (e, info) in ctx.combatants.iter() {
        // Must be alive ally; pets only where this dispel reaches them
        // (`ally_removal` — the healers' dispels do not).
        if info.team != my_team || info.current_health <= 0.0 || (info.is_pet && !reaches_pets) {
            continue;
        }
        if ctx.is_cycloned(*e) {
            continue;
        }
        let Some(ally_auras) = ctx.active_auras.get(e) else {
            continue;
        };

        // Highest priority debuff on this ally that the dispel takes.
        let highest_priority = ally_auras
            .iter()
            .filter(|aura| scope.takes(aura))
            .map(|aura| situational_ally_dispel_priority(ctx, info, aura))
            .max()
            .unwrap_or(-1);
        if highest_priority < min_priority || highest_priority < 0 {
            continue;
        }

        // Range, then line of sight — the gates every targeted cast passes.
        match cast_reach(ctx, range, my_pos, info.position) {
            CastReach::Reaches => {}
            CastReach::OutOfRange { distance } => {
                scan.nearest_out_of_range = Some(
                    scan.nearest_out_of_range
                        .map_or(distance, |d| d.min(distance)),
                );
                continue;
            }
            CastReach::LosBlocked => {
                scan.los_blocked = true;
                continue;
            }
        }

        if best.is_none_or(|(_, best_prio)| highest_priority > best_prio) {
            best = Some((*e, highest_priority));
        }
    }
    scan.target = best.map(|(e, _)| e);
    scan
}

/// Shared dispel logic used by Priest (Dispel Magic) and Paladin (Cleanse).
///
/// Finds the ally with the highest priority dispellable debuff and casts
/// the specified dispel ability on them. The actual aura removed is randomly
/// selected in process_dispels (WoW Classic behavior).
///
/// The `min_priority` parameter controls which debuffs are considered:
/// - 90: Only urgent CC (Polymorph, Freezing Trap's Incapacitate, the Unstable
///   Affliction Silence, Fear)
/// - 85 ([`ROOT_DISPEL_PRIORITY`]): also a root holding a teammate back
///   ([`root_holds_back`])
/// - 50: Include every root and DoTs
/// - 20: Include slows (not recommended)
///
/// Only an ally the dispel reaches is a candidate: in range AND in sight
/// ([`cast_reach`]), so a healer never dispels a teammate through a pillar.
///
/// Predicate failures emit typed reject events on the dispel ability;
/// success emits choose.
#[allow(clippy::too_many_arguments)]
pub fn try_dispel_ally(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    min_priority: i32,
    ability_type: AbilityType,
    log_prefix: &'static str,
    log_name: &str,
    caster_class: CharacterClass,
    trace: &mut crate::states::play_match::decision_trace::DecisionEventBuilder<'_>,
) -> bool {
    let def = abilities.get_unchecked(&ability_type);

    // The caster's own gates; the ally it reaches is the scan's below.
    let cleared = match cast_guard::cast_gate(
        ability_type,
        def,
        combatant,
        my_pos,
        auras,
        None,
        ctx,
        cast_guard::PreCastOpts::default(),
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            trace.reject(ability_type, reason);
            return false;
        }
    };

    let AllyRemoval { scope, .. } =
        ally_removal(ability_type).expect("the healers' dispels free allies");

    let AllyDispelScan {
        target: best_candidate,
        nearest_out_of_range,
        los_blocked,
    } = scan_ally_dispel(
        ctx,
        ability_type,
        combatant.team,
        my_pos,
        def.range,
        min_priority,
    );

    let Some(dispel_target) = best_candidate else {
        let reason = match nearest_out_of_range {
            _ if los_blocked => RejectionReason::LosBlocked,
            Some(distance) => RejectionReason::OutOfRange {
                distance,
                max: def.range,
            },
            None => RejectionReason::NoValidTarget,
        };
        trace.reject(ability_type, reason);
        return false;
    };

    trace.choose(cleared, Some(dispel_target), true);

    // Execute the ability
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    // Log
    let target_tuple = ctx.combatants.get(&dispel_target).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        log_name,
        target_tuple,
        "casts",
    );

    // Spawn pending dispel
    commands.spawn(DispelPending {
        target: dispel_target,
        dispeller: entity,
        log_prefix,
        caster_class,
        heal_on_success: None,
        scope,
    });

    info!(
        "Team {} {} casts {} on ally",
        combatant.team,
        combatant.class.name(),
        log_name
    );

    true
}

/// Offensive dispel: the Shaman's Purge. Structural mirror of
/// [`try_dispel_ally`], but aimed at an ENEMY's beneficial,
/// [`Aura::can_be_purged`] auras.
///
/// The AI decides only WHETHER to purge and on WHOM ([`select_purge`]: an
/// enemy carrying a buff worth at least `min_priority` whose random pick is
/// [`PurgeOdds::favourable`]). It never chooses the buff: the spawned
/// `DispelPending` is scoped [`DispelScope::Purge`], every purgeable buff, and
/// `process_dispels` takes one of them at random.
///
/// Gated by [`pre_cast_ok`] with `check_friendly_cc: false` (offensive — no
/// friendly-CC concern) and `check_target_immune: true` (respect Divine Shield
/// and Cyclone; range/mana/lockout/silence handled by the guard). Predicate
/// failures emit typed reject events — a purge held on its odds is
/// `PreconditionUnmet` with the odds in the note; success emits choose.
#[allow(clippy::too_many_arguments)]
pub fn try_purge_enemy(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    min_priority: i32,
    trace: &mut crate::states::play_match::decision_trace::DecisionEventBuilder<'_>,
) -> bool {
    use self::cast_guard::{cast_gate, unreached_reason, PreCastOpts};

    let ability = AbilityType::Purge;
    let def = abilities.get_unchecked(&ability);
    // Offensive cast — no friendly-CC guard.
    let opts = PreCastOpts {
        check_friendly_cc: false,
        check_friendly_dots: false,
        check_target_immune: true,
        bypass_silence: false,
    };

    // Value floor: `min_priority` is PURGE_MIN_PRIORITY for the ordinary purge
    // (only a target carrying a high-value defensive or sustain buff — cheap
    // re-buffs like Fortitude aren't worth the cast), PURGE_URGENT_PRIORITY
    // for the urgent one. The odds of the random pick are select_purge's too.
    let PurgeChoice {
        target: target_entity,
        position: target_pos,
        ..
    } = match select_purge(
        ctx,
        abilities,
        combatant.team,
        my_pos,
        def.range,
        min_priority,
    ) {
        Ok(choice) => choice,
        Err(reason) => {
            trace.reject(
                ability,
                unreached_reason(reason, ability, def, combatant, my_pos, auras, ctx, opts),
            );
            return false;
        }
    };

    // Universal pre-cast guard (lockout / silence / cooldown / mana / range /
    // target immunity).
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            trace.reject(ability, reason);
            return false;
        }
    };

    trace.choose(cleared, Some(target_entity), true);

    // Execute.
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    if def.cooldown > 0.0 {
        combatant.start_cooldown(ability, abilities);
    }

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        target_tuple,
        "casts",
    );

    // Any purgeable buff: process_dispels picks the one that comes off, at
    // random (intentional — see process_dispels).
    commands.spawn(DispelPending {
        target: target_entity,
        dispeller: entity,
        log_prefix: "[PURGE]",
        caster_class: combatant.class,
        heal_on_success: None,
        scope: DispelScope::Purge,
    });

    true
}

#[cfg(test)]
mod dispel_reach_tests {
    use super::*;
    use crate::states::play_match::abilities::SpellSchool;
    use crate::states::play_match::components::DispelType;

    fn aura(effect_type: AuraType, school: Option<SpellSchool>, class: DispelType) -> Aura {
        Aura {
            effect_type,
            spell_school: school,
            dispel_type: class,
            ..Default::default()
        }
    }

    /// The per-class removal truth, asserted against the ENGINE's predicates
    /// rather than against a comment: a dispel removes exactly MAGIC; a cleanse
    /// removes magic AND poison/disease; NOTHING removes a curse or a physical
    /// debuff.
    ///
    /// The Curse row is the one worth staring at. Curse of Agony is a Shadow
    /// damage-over-time effect, so before it declared its class it was removed
    /// by both a dispel and a cleanse — identical in every respect the engine
    /// looked at to Corruption, which SHOULD come off.
    #[test]
    fn dispel_reach_matches_the_removal_model() {
        let corruption = aura(
            AuraType::DamageOverTime,
            Some(SpellSchool::Shadow),
            DispelType::Auto,
        );
        let frost_nova = aura(AuraType::Root, Some(SpellSchool::Frost), DispelType::Auto);
        let crippling = aura(
            AuraType::MovementSpeedSlow,
            Some(SpellSchool::Nature),
            DispelType::Poison,
        );
        let agony = aura(
            AuraType::DamageOverTime,
            Some(SpellSchool::Shadow),
            DispelType::Curse,
        );
        let concussive = aura(AuraType::MovementSpeedSlow, None, DispelType::Physical);
        let rend = aura(AuraType::DamageOverTime, None, DispelType::Physical);

        // (ability, magic, poison, curse, physical)
        let expected = [
            (AbilityType::DispelMagic, true, false, false, false),
            (AbilityType::PaladinCleanse, true, true, false, false),
            (AbilityType::DevourMagic, true, false, false, false),
        ];

        for (ability, magic, poison, curse, physical) in expected {
            // The scope `process_dispels` applies for this ability, so the
            // matrix tests the real predicate and not a paraphrase of it.
            let scope = ally_dispel_scope(ability);
            let removes = |a: &Aura| scope.takes(a);

            assert_eq!(
                removes(&corruption),
                magic,
                "{ability:?} vs Corruption (magic)"
            );
            assert_eq!(
                removes(&frost_nova),
                magic,
                "{ability:?} vs Frost Nova (magic)"
            );
            assert_eq!(
                removes(&crippling),
                poison,
                "{ability:?} vs Crippling Poison"
            );
            assert_eq!(removes(&agony), curse, "{ability:?} vs Curse of Agony");
            assert_eq!(
                removes(&concussive),
                physical,
                "{ability:?} vs Concussive Shot"
            );
            assert_eq!(removes(&rend), physical, "{ability:?} vs Rend");
        }
    }

    /// Every `is_dispel` ability is classified by [`ally_removal`] BY NAME, and
    /// the classification is pinned: the named set is asserted EQUAL to the
    /// config's dispels, so a new dispel fails here until someone decides
    /// whether it frees allies, and a renamed one cannot fall to the `_` arm.
    #[test]
    fn ally_removal_names_every_dispel() {
        let defs = AbilityDefinitions::default();
        let mut dispels: Vec<AbilityType> = defs
            .iter()
            .filter(|(_, def)| def.is_dispel)
            .map(|(ability, _)| *ability)
            .collect();
        dispels.sort_by_key(|a| format!("{a:?}"));
        assert_eq!(
            dispels,
            vec![
                AbilityType::DevourMagic,
                AbilityType::DispelMagic,
                AbilityType::MastersCall,
                AbilityType::PaladinCleanse,
                AbilityType::Purge,
            ],
            "a dispel was added or renamed: classify it in `ally_removal`"
        );
        let freeing: Vec<(AbilityType, bool)> = dispels
            .iter()
            .filter_map(|a| ally_removal(*a).map(|r| (*a, r.reaches_pets)))
            .collect();
        assert_eq!(
            freeing,
            vec![
                (AbilityType::DevourMagic, true),
                (AbilityType::DispelMagic, false),
                (AbilityType::MastersCall, false),
                (AbilityType::PaladinCleanse, false),
            ]
        );
    }

    /// Only the Cleanse reaches poison. Scans the whole ability config so a new
    /// dispel cannot quietly inherit `false` unnoticed.
    #[test]
    fn only_the_cleanse_removes_poison() {
        let defs = AbilityDefinitions::default();
        let mut reaching: Vec<AbilityType> = defs
            .iter()
            .map(|(ability, _)| *ability)
            .filter(|a| dispel_removes_poison(*a))
            .collect();
        reaching.sort_by_key(|a| format!("{a:?}"));
        assert_eq!(reaching, vec![AbilityType::PaladinCleanse]);
    }
}

/// The scene the reach-first heal probes share (AS-202): the caster at the
/// origin; `BEHIND` 16yd down +X behind a pillar centred 8yd out; `IN_SIGHT`
/// 16yd down +Z with a clear line. Both are in every heal's range and in Holy
/// Shock's 20yd damage range, so only sight separates them.
#[cfg(test)]
pub(crate) mod reach_fixture {
    use super::*;
    use crate::states::play_match::components::DRTracker;
    use crate::states::play_match::decision_trace::DecisionTrace;

    pub const CASTER: Vec3 = Vec3::new(0.0, 1.0, 0.0);
    pub const BEHIND: Vec3 = Vec3::new(16.0, 1.0, 0.0);
    pub const IN_SIGHT: Vec3 = Vec3::new(0.0, 1.0, 16.0);

    static NO_DR: BTreeMap<Entity, DRTracker> = BTreeMap::new();
    static NO_CDS: BTreeMap<Entity, BTreeMap<AbilityType, f32>> = BTreeMap::new();

    pub fn unit(
        entity: Entity,
        team: u8,
        class: CharacterClass,
        position: Vec3,
        health_pct: f32,
    ) -> CombatantInfo {
        CombatantInfo {
            entity,
            team,
            slot: 0,
            class,
            current_health: 100.0 * health_pct,
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

    pub fn roster(units: Vec<CombatantInfo>) -> BTreeMap<Entity, CombatantInfo> {
        units.into_iter().map(|u| (u.entity, u)).collect()
    }

    /// The pillar between the caster and `BEHIND`, and nothing else.
    pub fn pillar() -> Vec<ObstacleVolume> {
        vec![ObstacleVolume::Cylinder {
            center_xz: Vec2::new(8.0, 0.0),
            radius: 2.0,
            base_y: 0.0,
            height: 10.0,
        }]
    }

    pub fn ctx<'a>(
        me: Entity,
        roster: &'a BTreeMap<Entity, CombatantInfo>,
        auras: &'a BTreeMap<Entity, Vec<Aura>>,
        obstacles: &'a [ObstacleVolume],
    ) -> CombatContext<'a> {
        CombatContext::new(
            me,
            roster[&me].team,
            roster,
            auras,
            &NO_DR,
            &NO_CDS,
            obstacles,
            Default::default(),
            Default::default(),
        )
    }

    /// A caster on team 1 at [`CASTER`] and the units around it. `units[0]` is
    /// the caster; the rest follow in the order given.
    pub struct ReachScene {
        pub world: World,
        pub units: Vec<Entity>,
        pub roster: BTreeMap<Entity, CombatantInfo>,
        pub auras: BTreeMap<Entity, Vec<Aura>>,
        pub combatant: Combatant,
        /// Applied to the fresh caster at the start of every [`Self::run`]:
        /// the caster's own state for a probe (no mana, a cooldown).
        pub prep: Option<fn(&mut Combatant)>,
        /// The clock the run's situation is classified at (default 20s after
        /// the gates).
        pub clock: MatchClock,
        caster: CharacterClass,
    }

    impl ReachScene {
        /// `others`: (team, class, position, health fraction) per unit.
        pub fn new(caster: CharacterClass, others: &[(u8, CharacterClass, Vec3, f32)]) -> Self {
            let mut world = World::new();
            let me = world.spawn_empty().id();
            let mut units = vec![me];
            let mut infos = vec![unit(me, 1, caster, CASTER, 1.0)];
            for &(team, class, position, hp) in others {
                let e = world.spawn_empty().id();
                units.push(e);
                infos.push(unit(e, team, class, position, hp));
            }
            ReachScene {
                world,
                units,
                roster: roster(infos),
                auras: BTreeMap::new(),
                combatant: Combatant::new(1, 0, caster),
                prep: None,
                clock: MatchClock::after_gates(20.0),
                caster,
            }
        }

        /// One decision by a fresh caster (off every cooldown) over the scene,
        /// with `obstacles`: `decide` runs a `try_*` against a fresh trace, and
        /// its commands are applied.
        // The closure's bound sits in a `where` clause so the registration
        // audit, which reads parameter lists, does not take this test helper
        // for a Bevy system that takes `Commands`.
        pub fn run<F>(&mut self, obstacles: &[ObstacleVolume], decide: F) -> DecisionTrace
        where
            F: FnOnce(
                &mut Commands,
                &CombatContext,
                &mut Combatant,
                &mut crate::states::play_match::decision_trace::DecisionEventBuilder,
            ) -> bool,
        {
            let me = self.units[0];
            // Every run starts off the global cooldown and every cooldown.
            self.combatant = Combatant::new(1, 0, self.caster);
            if let Some(prep) = self.prep {
                prep(&mut self.combatant);
            }
            let mut trace = DecisionTrace::default();
            let mut queue = bevy::ecs::world::CommandQueue::default();
            {
                let ctx = ctx(me, &self.roster, &self.auras, obstacles)
                    .classified(&SituationConfig::default(), self.clock);
                let mut commands = Commands::new(&mut queue, &self.world);
                let mut builder = ctx
                    .start_ability_decision(&mut trace, None, CASTER)
                    .expect("the caster is in its own roster");
                decide(&mut commands, &ctx, &mut self.combatant, &mut builder);
                builder.finish();
            }
            queue.apply(&mut self.world);
            trace
        }
    }

    /// A caster's auras carrying a Silence: a refusal no check ahead of the
    /// target pick reports (the pick precedes `pre_cast_ok`).
    pub fn silenced() -> ActiveAuras {
        ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::Silence,
                ..Default::default()
            }],
        }
    }

    /// The chosen ability and its target's id in the first decision of `trace`.
    pub fn outcome(trace: &DecisionTrace) -> Option<(String, Option<u64>)> {
        let event = serde_json::to_value(&trace.pending_events[0]).unwrap();
        event["outcome"]["ability"]
            .as_str()
            .map(|a| (a.to_string(), event["outcome"]["target_id"].as_u64()))
    }

    /// `ability` chosen on `target`, as [`outcome`] reports it.
    pub fn chose(ability: &str, target: Entity) -> Option<(String, Option<u64>)> {
        Some((ability.to_string(), Some(target.index() as u64)))
    }

    /// The traced candidate for `ability` in the first decision of `trace`.
    pub fn candidate(trace: &DecisionTrace, ability: &str) -> serde_json::Value {
        let event = serde_json::to_value(&trace.pending_events[0]).unwrap();
        event["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["ability"] == ability)
            .cloned()
            .unwrap_or_else(|| panic!("no {ability} candidate in {event}"))
    }
}
