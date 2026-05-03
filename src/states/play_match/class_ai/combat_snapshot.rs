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

use std::collections::HashMap;

use bevy::prelude::*;

use super::{CombatContext, CombatantInfo};
use crate::states::play_match::auras::reflect_instant_cc_in_snapshot;
use crate::states::play_match::components::{
    ActiveAuras, Aura, CastingState, ChannelingState, Combatant, DRTracker, Pet,
};

/// Per-frame snapshot of every combatant's stats, auras, and DR state.
///
/// Three maps keyed by `Entity`. Construction is via [`CombatSnapshot::from_queries`];
/// in tests, the struct can be built directly from `HashMap` literals.
pub struct CombatSnapshot {
    pub combatants: HashMap<Entity, CombatantInfo>,
    pub active_auras: HashMap<Entity, Vec<Aura>>,
    pub dr_trackers: HashMap<Entity, DRTracker>,
}

impl CombatSnapshot {
    /// Build a snapshot from the live Bevy queries that `decide_abilities`
    /// already holds.
    ///
    /// The split across three aura sources is required because casting and
    /// channeling combatants are excluded from the main `aura_query` (Bevy
    /// borrow-checker rules forbid simultaneous mutable + immutable access to
    /// the same component). Together the three queries cover every entity.
    ///
    /// Each `&Query<...>` is a shared borrow of a query the caller still owns
    /// mutably — we only need read access here, and the caller resumes its
    /// `.iter_mut()` after this call returns.
    pub fn build(
        aura_query: &Query<
            (Entity, &mut Combatant, &Transform, Option<&mut ActiveAuras>),
            (Without<CastingState>, Without<ChannelingState>),
        >,
        casting_auras: &Query<(Entity, &ActiveAuras), With<CastingState>>,
        channeling_auras: &Query<(Entity, &ActiveAuras), (With<ChannelingState>, Without<CastingState>)>,
        dr_tracker_query: &Query<(Entity, &DRTracker)>,
        pet_query: &Query<&Pet>,
    ) -> Self {
        let mut combatants: HashMap<Entity, CombatantInfo> = HashMap::new();
        let mut active_auras: HashMap<Entity, Vec<Aura>> = HashMap::new();

        for (entity, combatant, transform, auras_opt) in aura_query.iter() {
            let pet_comp = pet_query.get(entity).ok();
            combatants.insert(entity, CombatantInfo {
                entity,
                team: combatant.team,
                slot: combatant.slot,
                class: combatant.class,
                current_health: combatant.current_health,
                max_health: combatant.max_health,
                current_mana: combatant.current_mana,
                max_mana: combatant.max_mana,
                position: transform.translation,
                is_alive: combatant.is_alive(),
                stealthed: combatant.stealthed,
                target: combatant.target,
                is_pet: pet_comp.is_some(),
                pet_type: pet_comp.map(|p| p.pet_type),
            });
            if let Some(auras) = auras_opt {
                active_auras.insert(entity, auras.auras.clone());
            }
        }

        for (entity, auras) in casting_auras.iter() {
            active_auras.insert(entity, auras.auras.clone());
        }
        for (entity, auras) in channeling_auras.iter() {
            active_auras.insert(entity, auras.auras.clone());
        }

        let dr_trackers: HashMap<Entity, DRTracker> = dr_tracker_query
            .iter()
            .map(|(entity, tracker)| (entity, tracker.clone()))
            .collect();

        Self { combatants, active_auras, dr_trackers }
    }

    /// Borrow a `CombatContext` view of this snapshot for the given combatant.
    ///
    /// Cheap — just hands out three `&` references and copies one `Entity`.
    pub fn context_for(&self, self_entity: Entity) -> CombatContext<'_> {
        CombatContext {
            combatants: &self.combatants,
            active_auras: &self.active_auras,
            dr_trackers: &self.dr_trackers,
            self_entity,
        }
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
