//! Combat systems
//!
//! ECS systems that process combat logic.

use bevy::prelude::*;

use super::components::*;
use super::events::*;
use super::log::{CombatLog, CombatLogEventType};

/// Process damage events and apply damage to targets
pub fn process_damage_events(
    mut damage_events: EventReader<DamageEvent>,
    mut health_query: Query<&mut Health>,
    mut stats_query: Query<&mut MatchStats>,
) {
    for event in damage_events.read() {
        // Apply damage to target
        if let Ok(mut health) = health_query.get_mut(event.target) {
            health.current = (health.current - event.final_amount).max(0.0);
        }

        // Update source stats
        if let Ok(mut stats) = stats_query.get_mut(event.source) {
            stats.damage_done += event.final_amount;
        }

        // Update target stats
        if let Ok(mut stats) = stats_query.get_mut(event.target) {
            stats.damage_received += event.final_amount;
        }
    }
}

/// Process healing events and apply healing to targets
pub fn process_healing_events(
    mut healing_events: EventReader<HealingEvent>,
    mut health_query: Query<&mut Health>,
    mut stats_query: Query<&mut MatchStats>,
) {
    for event in healing_events.read() {
        // Apply healing to target
        if let Ok(mut health) = health_query.get_mut(event.target) {
            let actual_healing = (health.current + event.amount).min(health.maximum) - health.current;
            health.current += actual_healing;

            // Update source stats with actual healing (not overhealing)
            if let Ok(mut stats) = stats_query.get_mut(event.source) {
                stats.healing_done += actual_healing;
            }

            // Update target stats
            if let Ok(mut stats) = stats_query.get_mut(event.target) {
                stats.healing_received += actual_healing;
            }
        }
    }
}

/// Update aura durations and remove expired auras
pub fn update_aura_durations(
    mut commands: Commands,
    time: Res<Time>,
    mut aura_query: Query<(Entity, &mut Aura)>,
    mut aura_removed_events: EventWriter<AuraRemovedEvent>,
) {
    for (entity, mut aura) in aura_query.iter_mut() {
        if let Some(ref mut remaining) = aura.remaining_duration {
            *remaining -= time.delta_secs();
            if *remaining <= 0.0 {
                // Aura expired - get the parent entity and remove the aura
                aura_removed_events.send(AuraRemovedEvent {
                    target: entity, // This should be the parent, will need adjustment
                    aura_name: aura.name.clone(),
                    reason: super::events::AuraRemovalReason::Expired,
                });
                commands.entity(entity).despawn();
            }
        }
    }
}

/// Check for combatant deaths and send death events
pub fn check_combatant_deaths(
    health_query: Query<(Entity, &Health, &Combatant), Changed<Health>>,
    mut death_events: EventWriter<CombatantDeathEvent>,
) {
    for (entity, health, _combatant) in health_query.iter() {
        if health.is_dead() {
            // For now, we don't track the killer properly
            // This will need enhancement when we track damage sources
            death_events.send(CombatantDeathEvent {
                victim: entity,
                killer: entity, // Placeholder - should track last damage source
            });
        }
    }
}

/// Record events to the combat log
pub fn record_combat_log(
    mut combat_log: ResMut<CombatLog>,
    time: Res<Time>,
    mut damage_events: EventReader<DamageEvent>,
    mut healing_events: EventReader<HealingEvent>,
    mut death_events: EventReader<CombatantDeathEvent>,
    combatant_query: Query<&Combatant>,
) {
    // Update match time
    combat_log.match_time += time.delta_secs();

    // Log damage events
    for event in damage_events.read() {
        let source_name = combatant_query
            .get(event.source)
            .map(|c| c.name.as_str())
            .unwrap_or("Unknown");
        let target_name = combatant_query
            .get(event.target)
            .map(|c| c.name.as_str())
            .unwrap_or("Unknown");
        
        let ability = event.ability_name.as_deref().unwrap_or("Auto Attack");
        let crit = if event.is_critical { " (Critical)" } else { "" };
        
        let message = format!(
            "{}'s {} hits {} for {:.0} {:?} damage{}",
            source_name, ability, target_name, event.final_amount, event.damage_type, crit
        );
        
        combat_log.log(CombatLogEventType::Damage, message);
    }

    // Log healing events
    for event in healing_events.read() {
        let source_name = combatant_query
            .get(event.source)
            .map(|c| c.name.as_str())
            .unwrap_or("Unknown");
        let target_name = combatant_query
            .get(event.target)
            .map(|c| c.name.as_str())
            .unwrap_or("Unknown");
        
        let crit = if event.is_critical { " (Critical)" } else { "" };
        
        let message = format!(
            "{}'s {} heals {} for {:.0}{}",
            source_name, event.ability_name, target_name, event.amount, crit
        );
        
        combat_log.log(CombatLogEventType::Healing, message);
    }

    // Log death events
    for event in death_events.read() {
        let victim_name = combatant_query
            .get(event.victim)
            .map(|c| c.name.as_str())
            .unwrap_or("Unknown");
        let killer_name = combatant_query
            .get(event.killer)
            .map(|c| c.name.as_str())
            .unwrap_or("Unknown");
        
        let message = format!("{} has been slain by {}", victim_name, killer_name);
        combat_log.log(CombatLogEventType::Death, message);
    }
}

