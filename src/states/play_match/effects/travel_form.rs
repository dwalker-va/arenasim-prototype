//! Travel Form Effect Processing
//!
//! Processes the Druid's escape shift in both directions. Shifting INTO Travel
//! Form breaks every root and snare on the Druid — the reusable
//! [`ActiveAuras::break_movement_impairments`] — and puts the form on it (its
//! speed, its Polymorph immunity, and the casting block, all read off the
//! `TravelForm` aura). Shifting OUT removes the form.
//!
//! Uses the deferred pending pattern (like Divine Shield and Berserker Rage)
//! because the Druid AI has immutable aura access. Runs BEFORE
//! `apply_pending_auras`, so a Polymorph queued for the frame the Druid shifts
//! already meets the immunity, and a root queued for that frame lands after
//! the break rather than being swept by it.

use bevy::prelude::*;
use bevy_egui::egui;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::utils::{combatant_id, get_next_fct_offset};

/// Process pending Travel Form shifts.
pub fn process_travel_form(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    abilities: Res<AbilityDefinitions>,
    pending_shifts: Query<(Entity, &ShapeshiftPending)>,
    mut combatants: Query<(&Combatant, &Transform, Option<&mut ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
) {
    for (pending_entity, pending) in pending_shifts.iter() {
        commands.entity(pending_entity).despawn();
        let Ok((combatant, transform, active_auras)) = combatants.get_mut(pending.caster) else {
            continue;
        };
        if !combatant.is_alive() {
            continue;
        }
        let caster_id = combatant_id(combatant.team, combatant.slot, combatant.class);

        match pending.shift {
            Shift::IntoTravelForm => {
                let Some(pending_aura) = AuraPending::from_ability(
                    pending.caster,
                    pending.caster,
                    abilities.get_unchecked(&AbilityType::TravelForm),
                ) else {
                    continue;
                };
                let form = pending_aura.aura;

                let broken = if let Some(mut auras) = active_auras {
                    let broken = auras.break_movement_impairments();
                    auras.auras.push(form);
                    broken
                } else {
                    // No auras yet — insert the component with the form.
                    // `.chain()` auto-inserts ApplyDeferred, so this is
                    // visible to apply_pending_auras.
                    commands
                        .entity(pending.caster)
                        .insert(ActiveAuras { auras: vec![form] });
                    Vec::new()
                };

                combat_log.log(
                    CombatLogEventType::Buff,
                    format!("{} shifts into Travel Form", caster_id),
                );
                if !broken.is_empty() {
                    combat_log.log(
                        CombatLogEventType::Buff,
                        format!("{}'s Travel Form breaks {}", caster_id, broken.join(", ")),
                    );
                }

                // White status text per the color budget; the label carries it.
                let text_position =
                    transform.translation + Vec3::new(0.0, super::super::FCT_HEIGHT, 0.0);
                let (offset_x, offset_y) =
                    if let Ok(mut fct_state) = fct_states.get_mut(pending.caster) {
                        get_next_fct_offset(&mut fct_state)
                    } else {
                        (0.0, 0.0)
                    };
                commands.spawn((
                    FloatingCombatText {
                        world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                        text: "Travel Form".to_string(),
                        color: egui::Color32::WHITE,
                        lifetime: 2.0,
                        vertical_offset: offset_y,
                        is_crit: false,
                    },
                    PlayMatchEntity,
                ));
            }
            Shift::Out => {
                let Some(mut auras) = active_auras else {
                    continue;
                };
                let before = auras.auras.len();
                auras
                    .auras
                    .retain(|a| a.effect_type != AuraType::TravelForm);
                if auras.auras.len() != before {
                    combat_log.log(
                        CombatLogEventType::Buff,
                        format!("{} leaves Travel Form", caster_id),
                    );
                }
            }
        }
    }
}
