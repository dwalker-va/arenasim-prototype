//! Per-frame view of combat state used by class AI decisions.
//!
//! `CombatSnapshot` consolidates the three lookup maps that every class AI's
//! `try_*` function reads through `CombatContext`:
//! - `combatants` — read-only stat/position/team summary, indexed by Entity
//! - `active_auras` — current auras per entity, mutable during dispatch so
//!   instant-CCs landed by earlier combatants are visible to later ones
//! - `dr_trackers` — diminishing-returns state per entity, also mutated as
//!   instant CCs land
//!
//! The snapshot is built once at the start of `decide_abilities` from ECS
//! queries, then mutated in-place via [`CombatSnapshot::reflect_instant_cc`]
//! as each combatant's class AI runs. Per-frame ephemeral queues
//! (`shielded_this_frame`, `same_frame_cc_queue`, etc.) are NOT part of the
//! snapshot — they are dispatch-local accumulators owned by `decide_abilities`.

use std::collections::BTreeMap;

use bevy::prelude::*;

use super::{CombatContext, CombatantInfo};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ai_profile::AiProfiles;
use crate::states::play_match::arena_bounds::ArenaBounds;
use crate::states::play_match::auras::reflect_instant_cc_in_snapshot;
use crate::states::play_match::combat_core::MeleeDefense;
use crate::states::play_match::components::{
    ActiveAuras, Aura, CastingState, ChannelingState, ChargingState, Combatant, DRTracker,
    DisengagingState, Pet,
};
use crate::states::play_match::map_geometry::ObstacleVolume;

impl CombatantInfo {
    /// The snapshot row for one combatant: its live `Combatant` and
    /// `Transform`, the ability it is casting or channeling, its `Pet`
    /// component if it is one, and the pet it owns if it owns one. The one
    /// place a row is derived from components, shared by
    /// [`CombatSnapshot::build`] and the scene harness.
    pub fn from_combatant(
        entity: Entity,
        combatant: &Combatant,
        transform: &Transform,
        casting_ability: Option<AbilityType>,
        pet: Option<&Pet>,
        owned_pet: Option<Entity>,
    ) -> Self {
        // Estimated planar velocity: heading (Transform faces travel
        // direction) × base speed, zeroed while casting/channeling (planted).
        let velocity = if casting_ability.is_some() {
            Vec3::ZERO
        } else {
            let fwd = transform.rotation * Vec3::Z;
            Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero() * combatant.base_movement_speed
        };
        CombatantInfo {
            entity,
            team: combatant.team,
            slot: combatant.slot,
            class: combatant.class,
            current_health: combatant.current_health,
            max_health: combatant.max_health,
            current_mana: combatant.current_mana,
            max_mana: combatant.max_mana,
            position: transform.translation,
            velocity,
            is_alive: combatant.is_alive(),
            stealthed: combatant.stealthed,
            target: combatant.target,
            is_pet: pet.is_some(),
            pet_heeled: pet.is_some() && combatant.pet_heeled,
            casting_ability,
            pet_type: pet.map(|p| p.pet_type),
            pet: owned_pet,
            book: combatant.book().clone(),
        }
    }
}

/// One combatant as [`CombatSnapshot::from_units`] reads it: its components,
/// and the ability it is casting or channelling.
pub struct SnapshotUnit<'a> {
    pub entity: Entity,
    pub combatant: &'a Combatant,
    pub transform: &'a Transform,
    pub casting: Option<AbilityType>,
    pub auras: Option<&'a ActiveAuras>,
}

/// Per-frame snapshot of every combatant's stats, auras, and DR state.
///
/// Three maps keyed by `Entity`. Construction is via [`CombatSnapshot::build`];
/// in tests, the struct can be built directly from `BTreeMap` literals.
///
/// `BTreeMap` (rather than `HashMap`) is load-bearing for determinism: AI
/// modules iterate `combatants` for ties like "lowest-HP ally" or "nearest
/// enemy in range", and `HashMap` would resolve those ties via per-process
/// random hasher order — breaking seeded replay reproducibility.
pub struct CombatSnapshot {
    pub combatants: BTreeMap<Entity, CombatantInfo>,
    pub active_auras: BTreeMap<Entity, Vec<Aura>>,
    pub dr_trackers: BTreeMap<Entity, DRTracker>,
    /// Per-entity ability cooldowns. Hunter AI reads this when dispatching pet
    /// abilities — it needs to know the pet's cooldown state without holding a
    /// mutable handle to the pet `Combatant`. Cloned from `Combatant.ability_cooldowns`
    /// (which is a `HashMap`) into a `BTreeMap` for deterministic iteration.
    pub ability_cooldowns: BTreeMap<Entity, BTreeMap<AbilityType, f32>>,
    /// The active map's obstacle volumes (from `ActiveMapGeometry`), cloned in
    /// once per frame. Handed to every `CombatContext` this snapshot vends so
    /// the cast-start LoS guard can test caster→target sight. Empty on maps
    /// with no cover, making the guard a no-op there.
    pub obstacles: Vec<ObstacleVolume>,
    /// The active map's walkable region, captured alongside `obstacles` so every
    /// `CombatContext` derived from this snapshot sees the selected map's shape.
    pub bounds: ArenaBounds,
    /// Which AI implementation this match runs under.
    /// Per-TEAM, so a head-to-head match can run one implementation against
    /// the other. `context_for` resolves it to the acting unit's own team.
    pub ai_profile: AiProfiles,
    /// Every combatant with a dodge or parry chance above 0, and its chances.
    /// Empty in every match without avoidance: a melee attack on a combatant
    /// absent from it rolls nothing.
    pub melee_defense: BTreeMap<Entity, MeleeDefense>,
}

impl CombatSnapshot {
    /// Build a snapshot from the live Bevy queries that `decide_abilities`
    /// already holds.
    ///
    /// The split across three sources is required because casting and
    /// channeling combatants are excluded from the main `aura_query` (Bevy
    /// borrow-checker rules forbid simultaneous mutable + immutable access to
    /// the same component). Together the three queries cover every entity:
    /// casting/channeling combatants contribute full `CombatantInfo` entries
    /// (position, hp/mana, `.target`, stealth) just like idle ones, so class
    /// AI can see a casting ally's HP and a casting enemy's target.
    ///
    /// Each `&Query<...>` is a shared borrow of a query the caller still owns
    /// mutably — we only need read access here, and the caller resumes its
    /// `.iter_mut()` after this call returns.
    pub fn build(
        aura_query: &Query<
            (
                Entity,
                &mut Combatant,
                &Transform,
                Option<&mut ActiveAuras>,
                Option<&ChargingState>,
                Option<&DisengagingState>,
            ),
            (Without<CastingState>, Without<ChannelingState>),
        >,
        casting_auras: &Query<
            (
                Entity,
                &Combatant,
                &Transform,
                Option<&ActiveAuras>,
                &CastingState,
            ),
            With<CastingState>,
        >,
        channeling_auras: &Query<
            (
                Entity,
                &Combatant,
                &Transform,
                Option<&ActiveAuras>,
                &ChannelingState,
            ),
            (With<ChannelingState>, Without<CastingState>),
        >,
        dr_tracker_query: &Query<(Entity, &DRTracker)>,
        pet_query: &Query<&Pet>,
        obstacles: &[ObstacleVolume],
        bounds: ArenaBounds,
        ai_profile: AiProfiles,
    ) -> Self {
        let units = aura_query
            .iter()
            .map(|(entity, combatant, transform, auras, _, _)| SnapshotUnit {
                entity,
                combatant,
                transform,
                casting: None,
                auras,
            })
            .chain(
                casting_auras
                    .iter()
                    .map(|(entity, combatant, transform, auras, cast)| SnapshotUnit {
                        entity,
                        combatant,
                        transform,
                        casting: Some(cast.ability),
                        auras,
                    }),
            )
            .chain(channeling_auras.iter().map(
                |(entity, combatant, transform, auras, channel)| SnapshotUnit {
                    entity,
                    combatant,
                    transform,
                    casting: Some(channel.ability),
                    auras,
                },
            ));
        Self::from_units(
            units,
            pet_query,
            dr_tracker_query,
            obstacles,
            bounds,
            ai_profile,
        )
    }

    /// Build a snapshot from every combatant, each once: the one derivation
    /// [`Self::build`] and `check_interrupts` share. `units` may come in any
    /// order — every map is keyed and ordered by entity.
    pub fn from_units<'a>(
        units: impl Iterator<Item = SnapshotUnit<'a>>,
        pet_query: &Query<&Pet>,
        dr_tracker_query: &Query<(Entity, &DRTracker)>,
        obstacles: &[ObstacleVolume],
        bounds: ArenaBounds,
        ai_profile: AiProfiles,
    ) -> Self {
        let units: Vec<SnapshotUnit<'a>> = units.collect();
        // Owner→pet reverse lookup, so each combatant's `pet` field is set.
        // Casting/channeling pets are included so an owner's `pet` link
        // survives the pet being mid-cast.
        let owner_to_pet: BTreeMap<Entity, Entity> = units
            .iter()
            .filter_map(|u| {
                pet_query
                    .get(u.entity)
                    .ok()
                    .map(|pet| (pet.owner, u.entity))
            })
            .collect();

        let mut combatants: BTreeMap<Entity, CombatantInfo> = BTreeMap::new();
        let mut active_auras: BTreeMap<Entity, Vec<Aura>> = BTreeMap::new();
        let mut ability_cooldowns: BTreeMap<Entity, BTreeMap<AbilityType, f32>> = BTreeMap::new();
        let mut melee_defense: BTreeMap<Entity, MeleeDefense> = BTreeMap::new();
        for u in &units {
            let defense = u.combatant.melee_defense();
            if !defense.is_none() {
                melee_defense.insert(u.entity, defense);
            }
            combatants.insert(
                u.entity,
                CombatantInfo::from_combatant(
                    u.entity,
                    u.combatant,
                    u.transform,
                    u.casting,
                    pet_query.get(u.entity).ok(),
                    owner_to_pet.get(&u.entity).copied(),
                ),
            );
            // Clone Combatant.ability_cooldowns (HashMap) into a BTreeMap so
            // downstream iteration is deterministic.
            ability_cooldowns.insert(
                u.entity,
                u.combatant
                    .ability_cooldowns
                    .iter()
                    .map(|(k, v)| (*k, *v))
                    .collect(),
            );
            if let Some(auras) = u.auras {
                active_auras.insert(u.entity, auras.auras.clone());
            }
        }

        let dr_trackers: BTreeMap<Entity, DRTracker> = dr_tracker_query
            .iter()
            .map(|(entity, tracker)| (entity, tracker.clone()))
            .collect();

        Self {
            combatants,
            active_auras,
            dr_trackers,
            ability_cooldowns,
            obstacles: obstacles.to_vec(),
            bounds,
            ai_profile,
            melee_defense,
        }
    }

    /// Borrow a `CombatContext` view of this snapshot for the given combatant:
    /// what that combatant can perceive (see [`CombatContext::new`]).
    ///
    /// Cheap — hands out `&` references, and copies the combatant map only
    /// while an enemy is hidden from this observer.
    pub fn context_for(&self, self_entity: Entity) -> CombatContext<'_> {
        let team = self.combatants.get(&self_entity).map(|c| c.team);
        CombatContext::new(
            self_entity,
            team.unwrap_or_default(),
            &self.combatants,
            &self.active_auras,
            &self.dr_trackers,
            &self.ability_cooldowns,
            &self.obstacles,
            self.bounds,
            // The acting unit's OWN profile — this is what makes every
            // downstream `ctx.ai_profile.is_team_plan()` gate per-team.
            team.map(|t| self.ai_profile.for_team(t))
                .unwrap_or_default(),
        )
        .with_melee_defense(&self.melee_defense)
    }

    /// Mutate the snapshot to reflect an instant CC just landed by a class AI
    /// earlier in this frame's dispatch loop, so subsequent combatants see the
    /// CC immediately (closes the "Cheap Shot then Kick same-frame same-target"
    /// wasted-interrupt window).
    ///
    /// Delegates to [`reflect_instant_cc_in_snapshot`], which handles DR
    /// scaling, immunity rejection, and same-category CC replacement.
    pub fn reflect_instant_cc(&mut self, target: Entity, aura: &Aura) {
        reflect_instant_cc_in_snapshot(target, aura, &mut self.active_auras, &mut self.dr_trackers);
    }
}
