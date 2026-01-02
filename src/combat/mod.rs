//! Combat system
//!
//! Implements the core combat mechanics including:
//! - Combatant stats and resources (HP, Mana, Rage, Energy)
//! - Abilities and spells
//! - Buffs, debuffs, and auras
//! - Crowd control
//! - Combat logging

use bevy::prelude::*;

pub mod components;
pub mod events;
pub mod log;
pub mod systems;

use events::*;
use systems::*;

/// Plugin for the combat system
pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app
            // Combat events
            .add_event::<DamageEvent>()
            .add_event::<HealingEvent>()
            .add_event::<AbilityUsedEvent>()
            .add_event::<AuraAppliedEvent>()
            .add_event::<AuraRemovedEvent>()
            .add_event::<CrowdControlEvent>()
            .add_event::<CombatantDeathEvent>()
            // Resources
            .init_resource::<log::CombatLog>()
            .init_resource::<SimulationSpeed>()
            // Systems
            .add_systems(Update, (
                process_damage_events,
                process_healing_events,
                update_aura_durations,
                check_combatant_deaths,
                record_combat_log,
            ).chain());
    }
}

/// Controls the speed of the combat simulation
#[derive(Resource)]
pub struct SimulationSpeed {
    /// Speed multiplier (0.0 = paused, 0.5 = half speed, 1.0 = normal, 2.0 = double, 3.0 = triple)
    pub multiplier: f32,
}

impl Default for SimulationSpeed {
    fn default() -> Self {
        Self { multiplier: 1.0 }
    }
}

impl SimulationSpeed {
    pub fn pause(&mut self) {
        self.multiplier = 0.0;
    }

    pub fn half_speed(&mut self) {
        self.multiplier = 0.5;
    }

    pub fn normal_speed(&mut self) {
        self.multiplier = 1.0;
    }

    pub fn double_speed(&mut self) {
        self.multiplier = 2.0;
    }

    pub fn triple_speed(&mut self) {
        self.multiplier = 3.0;
    }

    pub fn is_paused(&self) -> bool {
        self.multiplier == 0.0
    }
}

