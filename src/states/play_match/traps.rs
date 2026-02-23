//! Trap and Slow Zone Systems
//!
//! Two systems handle the full Hunter trap lifecycle:
//! - `trap_system()` — arm timer, proximity trigger, effect application
//! - `slow_zone_system()` — zone duration tick, slow aura refresh

use bevy::prelude::*;
use crate::combat::log::{CombatLog, CombatLogEventType};
use super::components::*;
use super::constants::*;
use super::abilities::SpellSchool;

/// Single system handling the full trap lifecycle:
/// 1. Decrement arm_timer, consider armed when timer hits 0
/// 2. For armed traps: check proximity against enemy combatants and pets
/// 3. On trigger: apply effect (Incapacitate or spawn SlowZone), despawn trap
pub fn trap_system(
    mut commands: Commands,
    time: Res<Time>,
    mut combat_log: ResMut<CombatLog>,
    mut traps: Query<(Entity, &mut Trap, &Transform)>,
    combatants: Query<(Entity, &Combatant, &Transform), Without<Trap>>,
) {
    let dt = time.delta_secs();

    for (trap_entity, mut trap, trap_transform) in traps.iter_mut() {
        // Skip already triggered traps
        if trap.triggered {
            continue;
        }

        // Phase 1: Arm timer
        if trap.arm_timer > 0.0 {
            trap.arm_timer -= dt;
            if trap.arm_timer > 0.0 {
                continue; // Not armed yet, skip proximity check
            }
        }

        // Phase 2: Check proximity against enemy combatants
        let trap_pos = trap_transform.translation;
        let mut triggered_by: Option<(Entity, u8, String)> = None;

        for (target_entity, target_combatant, target_transform) in combatants.iter() {
            // Skip dead combatants
            if !target_combatant.is_alive() {
                continue;
            }

            // Skip friendly combatants (traps only trigger on enemies)
            if target_combatant.team == trap.owner_team {
                continue;
            }

            let distance = trap_pos.distance(target_transform.translation);
            if distance <= trap.trigger_radius {
                triggered_by = Some((
                    target_entity,
                    target_combatant.team,
                    format!("Team {} {}", target_combatant.team, target_combatant.class.name()),
                ));
                break; // First enemy in range triggers it
            }
        }

        // Phase 3: Apply effect if triggered
        if let Some((target_entity, _target_team, target_name)) = triggered_by {
            trap.triggered = true;
            let owner_name = format!("Team {}", trap.owner_team);

            match trap.trap_type {
                TrapType::Freezing => {
                    // Apply Incapacitate aura via AuraPending
                    commands.spawn((
                        AuraPending {
                            target: target_entity,
                            aura: Aura {
                                effect_type: AuraType::Incapacitate,
                                duration: 8.0,
                                magnitude: 0.0,
                                tick_interval: 0.0,
                                time_until_next_tick: 0.0,
                                caster: Some(trap.owner),
                                ability_name: "Freezing Trap".to_string(),
                                break_on_damage_threshold: 0.0, // Breaks on ANY damage
                                accumulated_damage: 0.0,
                                fear_direction: (0.0, 0.0),
                                fear_direction_timer: 0.0,
                                spell_school: Some(SpellSchool::Frost),
                            },
                        },
                        PlayMatchEntity,
                    ));

                    combat_log.log(
                        CombatLogEventType::CrowdControl,
                        format!(
                            "[TRAP] {}'s Freezing Trap triggers on {} — Incapacitated for 8 sec!",
                            owner_name, target_name
                        ),
                    );
                }
                TrapType::Frost => {
                    // Spawn a SlowZone entity at the trap location
                    commands.spawn((
                        Transform::from_translation(trap_pos),
                        SlowZone {
                            owner: trap.owner,
                            owner_team: trap.owner_team,
                            radius: FROST_TRAP_ZONE_RADIUS,
                            duration_remaining: FROST_TRAP_ZONE_DURATION,
                            slow_magnitude: 0.4, // 60% slow (magnitude = speed multiplier, 0.4 = 40% speed)
                        },
                        PlayMatchEntity,
                    ));

                    combat_log.log(
                        CombatLogEventType::CrowdControl,
                        format!(
                            "[TRAP] {}'s Frost Trap triggers on {} — slow zone created!",
                            owner_name, target_name
                        ),
                    );
                }
            }

            // Trap is consumed on trigger (even if target is immune)
            commands.entity(trap_entity).despawn();
        }
    }
}

/// Slow zone system handling zone lifecycle and aura refresh:
/// 1. Decrement duration_remaining, despawn when expired
/// 2. For enemies inside radius: refresh MovementSpeedSlow aura
pub fn slow_zone_system(
    mut commands: Commands,
    time: Res<Time>,
    mut zones: Query<(Entity, &mut SlowZone, &Transform)>,
    mut combatants: Query<(Entity, &Combatant, &Transform, Option<&mut ActiveAuras>), Without<SlowZone>>,
) {
    let dt = time.delta_secs();

    for (zone_entity, mut zone, zone_transform) in zones.iter_mut() {
        // Tick zone duration
        zone.duration_remaining -= dt;
        if zone.duration_remaining <= 0.0 {
            commands.entity(zone_entity).despawn();
            continue;
        }

        let zone_pos = zone_transform.translation;

        // Check all enemy combatants for proximity
        for (target_entity, target_combatant, target_transform, active_auras) in combatants.iter_mut() {
            // Skip dead combatants
            if !target_combatant.is_alive() {
                continue;
            }

            // Skip friendly combatants
            if target_combatant.team == zone.owner_team {
                continue;
            }

            let distance = zone_pos.distance(target_transform.translation);
            if distance <= zone.radius {
                // Enemy is inside zone — refresh or apply slow aura
                if let Some(mut auras) = active_auras {
                    // Look for existing Frost Trap slow aura
                    let existing_slow = auras.auras.iter_mut().find(|a| {
                        a.effect_type == AuraType::MovementSpeedSlow
                            && a.ability_name == "Frost Trap"
                    });

                    if let Some(slow) = existing_slow {
                        // Refresh duration (don't re-apply DR)
                        slow.duration = 2.0; // Short duration — refreshed each tick while in zone
                    } else {
                        // Apply new slow aura directly (no DR for zone-managed auras)
                        auras.auras.push(Aura {
                            effect_type: AuraType::MovementSpeedSlow,
                            duration: 2.0,
                            magnitude: zone.slow_magnitude,
                            tick_interval: 0.0,
                            time_until_next_tick: 0.0,
                            caster: Some(zone.owner),
                            ability_name: "Frost Trap".to_string(),
                            break_on_damage_threshold: -1.0, // Never breaks on damage
                            accumulated_damage: 0.0,
                            fear_direction: (0.0, 0.0),
                            fear_direction_timer: 0.0,
                            spell_school: Some(SpellSchool::Frost),
                        });
                    }
                } else {
                    // Target has no ActiveAuras yet — add component with slow
                    commands.entity(target_entity).try_insert(ActiveAuras {
                        auras: vec![Aura {
                            effect_type: AuraType::MovementSpeedSlow,
                            duration: 2.0,
                            magnitude: zone.slow_magnitude,
                            tick_interval: 0.0,
                            time_until_next_tick: 0.0,
                            caster: Some(zone.owner),
                            ability_name: "Frost Trap".to_string(),
                            break_on_damage_threshold: -1.0,
                            accumulated_damage: 0.0,
                            fear_direction: (0.0, 0.0),
                            fear_direction_timer: 0.0,
                            spell_school: Some(SpellSchool::Frost),
                        }],
                    });
                }
            }
        }
    }
}
