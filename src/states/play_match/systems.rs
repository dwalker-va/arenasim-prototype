//! Combat Systems API
//!
//! This module provides a stable API for the combat simulation systems.
//! Both graphical and headless modes should import from here rather than
//! directly from internal modules, allowing internal refactoring without
//! breaking external consumers.
//!
//! ## System Phases
//!
//! Combat systems run in three ordered phases each frame:
//!
//! 1. **ResourcesAndAuras** - Timer updates, resource regeneration, aura processing
//! 2. **CombatAndMovement** - Target acquisition, ability decisions, casting, projectiles
//! 3. **CombatResolution** - Auto-attacks, death checks, visual effects
//!
//! ## Usage
//!
//! ```ignore
//! use crate::states::play_match::systems::{self, CoreCombatSystems};
//!
//! // Add core combat systems to your app
//! systems::add_core_combat_systems(&mut app, in_state(GameState::PlayMatch));
//! ```

use bevy::prelude::*;

// Re-export all combat systems from internal modules
// This provides a stable API - internal renames only require updating these re-exports

// === Phase 1: Resources and Auras ===
pub use super::match_flow::update_countdown;
pub use super::team_plan::update_team_plans;
pub use super::match_flow::update_dampening;
pub use super::combat_core::regenerate_resources;
pub use super::shadow_sight::track_shadow_sight_timer;
pub use super::auras::process_dot_ticks;
pub use super::auras::process_hot_ticks;
pub use super::auras::update_auras;
pub use super::auras::apply_pending_auras;
// Effect processing (instant ability effects)
pub use super::effects::process_dispels;
pub use super::effects::process_holy_shock_heals;
pub use super::effects::process_holy_shock_damage;
pub use super::effects::process_divine_shield;
pub use super::effects::process_berserker_rage;
pub use super::effects::process_backlash;
pub use super::effects::process_mana_burn;

// === Phase 2: Combat and Movement ===
pub use super::auras::process_aura_breaks;
pub use super::combat_ai::acquire_targets;
pub use super::shadow_sight::check_orb_pickups;
pub use super::shadow_sight::cleanup_consumed_orbs;
pub use super::class_ai::dps_postures::tick_kite_occlusion;
pub use super::combat_ai::decide_abilities;
pub use super::class_ai::pet_ai::pet_ai_system;
pub use super::combat_ai::check_interrupts;
pub use super::combat_core::process_interrupts;
pub use super::combat_core::process_casting;
pub use super::combat_core::process_channeling;
pub use super::projectiles::move_projectiles;
pub use super::projectiles::process_projectile_hits;
pub use super::combat_core::move_to_target;
pub use super::traps::trap_system;
pub use super::traps::move_trap_launch_projectiles;
pub use super::combat_core::despawn_pets_of_dead_owners;

// === Phase 1 (additional): Slow Zone ===
pub use super::traps::slow_zone_system;

// === Phase 1 (additional): Totem pulse ===
pub use super::totems::totem_pulse_system;

// === Phase 3: Combat Resolution ===
pub use super::combat_core::combat_auto_attack;

// === Decision Trace ===
pub use super::decision_trace::flush_decision_trace_system;

// === Utilities ===
pub use super::utils::{combatant_id, pet_combatant_id};

// === Components and Resources ===
pub use super::components::{
    Combatant, CastingState, ChannelingState, ActiveAuras, Aura, AuraPending, AuraType,
    ArenaDampening, FloatingTextState, GameRng, MatchCountdown, SimulationSpeed, ShadowSightState,
};

/// System set labels for combat system ordering.
///
/// Use these to ensure proper ordering when adding custom systems that
/// interact with combat.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum CombatSystemPhase {
    /// Phase 1: Resource regeneration, DoT ticks, aura updates
    ResourcesAndAuras,
    /// Phase 2: Targeting, abilities, casting, projectiles, movement
    CombatAndMovement,
    /// Phase 3: Auto-attacks, death checks, match end
    CombatResolution,
}

/// Configures the ordering between combat system phases.
///
/// Call this once during app setup before adding combat systems.
pub fn configure_combat_system_ordering(app: &mut App) {
    // The simulation runs on a FIXED timestep, not per rendered frame.
    //
    // Combat systems consume `Time::delta_secs()` for movement, cooldowns and
    // casting, so running them in `Update` made the whole simulation frame-rate
    // dependent: the same seed produced a 165.65s match headless and 68.78s in
    // the graphical client, because headless pins ManualDuration(1/60) while the
    // client ran on whatever the GPU delivered. That silently voided the seed as a
    // reproduction handle and meant match outcomes varied with the player's
    // hardware.
    //
    // 1/60 matches the rate headless already forced, so headless behaviour is
    // unchanged (the recorded baseline is the proof); the client now steps the sim
    // at the same rate and catches up across frames instead of scaling with them.
    // Visual systems stay in `Update` — they should run per frame.
    app.insert_resource(Time::<Fixed>::from_hz(60.0));
    app.configure_sets(
        FixedUpdate,
        (
            CombatSystemPhase::ResourcesAndAuras,
            CombatSystemPhase::CombatAndMovement,
            CombatSystemPhase::CombatResolution,
        )
            .chain(),
    );
}

/// Adds core combat simulation systems to the app.
///
/// These are the systems needed for the combat loop to function.
/// Both graphical and headless modes need these.
///
/// ## Two layers, two conditions
///
/// The systems split into a **resolution** layer (auras, casting, projectiles,
/// effects, auto-attacks — everything downstream of a decision) and a
/// **decision** layer (target acquisition, ability choice, pet AI, interrupts,
/// AI movement, and the match clock). The Animation Sandbox needs the first and
/// must not run the second: it supplies the decision itself, and an AI fighting
/// underneath is precisely what makes an animation hard to judge.
///
/// The split is expressed as a per-system `run_if` INSIDE the existing
/// `.chain()` rather than as two separate registrations. That matters twice
/// over:
///
/// - `.chain()` ordering edges are structural, so per-system conditions cannot
///   reorder anything. Headless passes an always-true condition for both
///   layers, so every system runs exactly as before and the recorded baselines
///   stay valid.
/// - Registering the resolution systems a second time for the sandbox would
///   make their `SystemTypeSet`s ambiguous, and Bevy then refuses any
///   `.after()`/`.before()` that names one — `spawn_projectile_visuals` orders
///   against `process_channeling` and `move_projectiles`, so a second
///   registration panicked the app at schedule init.
///
/// # Arguments
/// * `app` - The Bevy App to add systems to
/// * `scene_condition` - where combat RESOLUTION runs
/// * `decision_condition` - where the AI and match clock ALSO run; must be a
///   subset of `scene_condition`
///
/// # Example
/// ```ignore
/// // Graphical: resolution in both combat scenes, decisions only in a match
/// add_core_combat_systems(&mut app, in_combat_scene, in_state(GameState::PlayMatch));
///
/// // Headless: everything, always
/// add_core_combat_systems(&mut app, || true, || true);
/// ```
pub fn add_core_combat_systems<M, N>(
    app: &mut App,
    scene_condition: impl Condition<M> + Clone,
    decision_condition: impl Condition<N> + Clone,
) where
    M: 'static,
    N: 'static,
{
    let run_condition = scene_condition;
    let decide = decision_condition;
    // Initialize DecisionTrace resource (idempotent — safe to call from both
    // headless and graphical setup paths).
    app.init_resource::<super::decision_trace::DecisionTrace>();

    // Phase 1: Resources and Auras
    app.add_systems(
        FixedUpdate,
        (
            update_countdown.run_if(decide.clone()),
            // Ramp heal/absorb dampening BEFORE any healing applies this frame
            update_dampening.run_if(decide.clone()),
            regenerate_resources,
            track_shadow_sight_timer.run_if(decide.clone()),
            process_dot_ticks,
            process_hot_ticks,     // HoT healing — like process_dot_ticks, must run BEFORE update_auras
            update_auras,
            slow_zone_system,       // Zone slow refresh before aura processing
            totem_pulse_system,     // Totem dedup + buff pulse on allies (after slow_zone_system)
            process_divine_shield,  // Must run BEFORE apply_pending_auras so DamageImmunity blocks CC
            process_berserker_rage, // Must run BEFORE apply_pending_auras so FearImmunity blocks queued Fears
            apply_pending_auras,
            process_dispels,
            // Must run AFTER process_dispels (consumes BacklashPending events that
            // process_dispels spawns) and in the same Phase 1 chain so backlash
            // damage + Silence land on the same frame as the dispel.
            process_backlash,
            process_holy_shock_heals,
            process_holy_shock_damage,
            process_mana_burn,
            // Team-level strategy. Recomputes on roster change; NOTHING reads the
            // result yet, so it is a no-op in both profiles. Under the default
            // `AiProfile::Legacy` it does not even select a plan, which is what
            // keeps the recorded behaviour baselines valid. Registered here so the
            // cadence and dual-mode wiring are exercised before any behaviour
            // depends on it.
            // Must never draw from GameRng: it shares this schedule with the AI,
            // so a draw would shift every downstream roll.
            update_team_plans.run_if(decide.clone()),
        )
            .chain()
            .in_set(CombatSystemPhase::ResourcesAndAuras)
            .run_if(run_condition.clone()),
    );

    // Flush deferred commands between phases
    app.add_systems(
        FixedUpdate,
        ApplyDeferred
            .after(CombatSystemPhase::ResourcesAndAuras)
            .before(CombatSystemPhase::CombatAndMovement)
            .run_if(run_condition.clone()),
    );

    // Phase 2: Combat and Movement
    app.add_systems(
        FixedUpdate,
        (
            process_aura_breaks,
            acquire_targets.run_if(decide.clone()),
            check_orb_pickups.run_if(decide.clone()),
            cleanup_consumed_orbs.run_if(decide.clone()),
            // Per-frame occlusion accumulator for the Mage/Hunter kiters. MUST
            // run before decide_abilities so the chase reads a fresh bucket, and
            // it ticks casting kiters too (they're excluded from decide_abilities'
            // query), capturing the mid-cast juke the ability pass never sees.
            tick_kite_occlusion.run_if(decide.clone()),
            decide_abilities.run_if(decide.clone()),
            ApplyDeferred, // Flush PetCommand components spawned by Hunter
                            // AI in decide_abilities so pet_ai_system sees
                            // them on the same tick (per U3 of the pet
                            // engagement plan). Without this, PetCommand has
                            // one-tick lag.
            pet_ai_system.run_if(decide.clone()),
            ApplyDeferred, // Flush CastingState for interrupt checks
            check_interrupts.run_if(decide.clone()),
            process_interrupts.run_if(decide.clone()),
            process_casting,
            process_channeling,
            move_projectiles,
            move_trap_launch_projectiles,  // Arc travel for launched traps — before trap_system
            process_projectile_hits,
            move_to_target.run_if(decide.clone()),
            trap_system,  // After movement — needs current positions for proximity check
            // Kill pets whose owner has died
            despawn_pets_of_dead_owners.run_if(decide.clone()),
        )
            .chain()
            .in_set(CombatSystemPhase::CombatAndMovement)
            .run_if(run_condition.clone()),
    );

    // Phase 3: Combat Resolution
    app.add_systems(
        FixedUpdate,
        (
            combat_auto_attack,
            flush_decision_trace_system.run_if(decide),
        )
            .chain()
            .in_set(CombatSystemPhase::CombatResolution)
            .run_if(run_condition),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_phase_ordering() {
        // Verify system phases can be compared for ordering
        assert_ne!(
            CombatSystemPhase::ResourcesAndAuras,
            CombatSystemPhase::CombatAndMovement
        );
        assert_ne!(
            CombatSystemPhase::CombatAndMovement,
            CombatSystemPhase::CombatResolution
        );
    }
}
