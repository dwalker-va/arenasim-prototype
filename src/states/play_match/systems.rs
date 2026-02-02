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
pub use super::combat_core::regenerate_resources;
pub use super::shadow_sight::track_shadow_sight_timer;
pub use super::auras::process_dot_ticks;
pub use super::auras::update_auras;
pub use super::auras::apply_pending_auras;
// Effect processing (instant ability effects)
pub use super::effects::process_dispels;
pub use super::effects::process_holy_shock_heals;
pub use super::effects::process_holy_shock_damage;

// === Phase 2: Combat and Movement ===
pub use super::auras::process_aura_breaks;
pub use super::combat_ai::acquire_targets;
pub use super::shadow_sight::check_orb_pickups;
pub use super::shadow_sight::animate_orb_consumption;
pub use super::combat_ai::decide_abilities;
pub use super::combat_ai::check_interrupts;
pub use super::combat_core::process_interrupts;
pub use super::combat_core::process_casting;
pub use super::combat_core::process_channeling;
pub use super::projectiles::move_projectiles;
pub use super::projectiles::process_projectile_hits;
pub use super::combat_core::move_to_target;

// === Phase 3: Combat Resolution ===
pub use super::combat_core::combat_auto_attack;

// === Utilities ===
pub use super::utils::combatant_id;

// === Components and Resources ===
pub use super::components::{
    Combatant, CastingState, ChannelingState, ActiveAuras, Aura, AuraPending, AuraType,
    FloatingTextState, GameRng, MatchCountdown, SimulationSpeed, ShadowSightState,
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
    app.configure_sets(
        Update,
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
/// # Arguments
/// * `app` - The Bevy App to add systems to
/// * `run_condition` - A run condition (e.g., `in_state(GameState::PlayMatch)`)
///
/// # Example
/// ```ignore
/// // For graphical mode
/// add_core_combat_systems(&mut app, in_state(GameState::PlayMatch));
///
/// // For headless mode (always run)
/// add_core_combat_systems(&mut app, || true);
/// ```
pub fn add_core_combat_systems<M>(app: &mut App, run_condition: impl Condition<M> + Clone)
where
    M: 'static,
{
    // Phase 1: Resources and Auras
    app.add_systems(
        Update,
        (
            update_countdown,
            regenerate_resources,
            track_shadow_sight_timer,
            process_dot_ticks,
            update_auras,
            apply_pending_auras,
            process_dispels,
            process_holy_shock_heals,
            process_holy_shock_damage,
        )
            .chain()
            .in_set(CombatSystemPhase::ResourcesAndAuras)
            .run_if(run_condition.clone()),
    );

    // Flush deferred commands between phases
    app.add_systems(
        Update,
        apply_deferred
            .after(CombatSystemPhase::ResourcesAndAuras)
            .before(CombatSystemPhase::CombatAndMovement)
            .run_if(run_condition.clone()),
    );

    // Phase 2: Combat and Movement
    app.add_systems(
        Update,
        (
            process_aura_breaks,
            acquire_targets,
            check_orb_pickups,
            animate_orb_consumption,
            decide_abilities,
            apply_deferred, // Flush CastingState for interrupt checks
            check_interrupts,
            process_interrupts,
            process_casting,
            process_channeling,
            move_projectiles,
            process_projectile_hits,
            move_to_target,
        )
            .chain()
            .in_set(CombatSystemPhase::CombatAndMovement)
            .run_if(run_condition.clone()),
    );

    // Phase 3: Combat Resolution
    app.add_systems(
        Update,
        combat_auto_attack
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
