//! Berserker Rage Effect Processing
//!
//! Processes Berserker Rage activation: breaks Fear auras on the Warrior and
//! applies the FearImmunity aura. Uses the BerserkerRagePending deferred pattern
//! because Warrior AI has immutable aura access (same as Divine Shield).
//!
//! TBC-faithful horror exception: Death Coil's horror is a Fear-type aura with
//! `dr_category_override: Some(DRCategory::Horror)`. It is NOT broken by
//! Berserker Rage and NOT blocked by FearImmunity — horror and fear are separate
//! mechanics, so Death Coil stays the Warlock's answer to a zerking Warrior.

use bevy::prelude::*;
use bevy_egui::egui;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::utils::{combatant_id, get_next_fct_offset};

/// Process pending Berserker Rage activations.
///
/// When a Warrior activates Berserker Rage, a BerserkerRagePending component is
/// spawned. This system removes breakable Fear auras (horror excluded), applies
/// the FearImmunity aura, and logs the activation. It must run BEFORE
/// apply_pending_auras so the immunity also blocks any Fear application already
/// queued for this frame.
pub fn process_berserker_rage(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    abilities: Res<AbilityDefinitions>,
    pending_rages: Query<(Entity, &BerserkerRagePending)>,
    mut combatants: Query<(&Combatant, &Transform, Option<&mut ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
) {
    // The immunity aura is data-driven from `abilities.ron` (BerserkerRage's
    // `applies_aura`) so it can be tuned without a recompile. Fallbacks
    // preserve the shipped values if the config is missing.
    let br_aura = abilities
        .get(&AbilityType::BerserkerRage)
        .and_then(|c| c.applies_aura.as_ref());
    let immunity_duration = br_aura.map(|a| a.duration).unwrap_or(10.0);
    let immunity_magnitude = br_aura.map(|a| a.magnitude).unwrap_or(1.0);
    let immunity_break = br_aura.map(|a| a.break_on_damage).unwrap_or(-1.0);

    for (pending_entity, pending) in pending_rages.iter() {
        if let Ok((combatant, transform, active_auras_opt)) = combatants.get_mut(pending.caster) {
            if !combatant.is_alive() {
                commands.entity(pending_entity).despawn();
                continue;
            }

            let immunity_aura = Aura {
                effect_type: AuraType::FearImmunity,
                duration: immunity_duration,
                magnitude: immunity_magnitude,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                break_on_damage_threshold: immunity_break, // -1.0 = never break on damage
                accumulated_damage: 0.0,
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
                caster: Some(pending.caster),
                ability_name: "Berserker Rage".to_string(),
                spell_school: None,
                applied_this_frame: false,
                backlash_damage: None,
                dr_category_override: None,
                dispel_type: DispelType::Auto,
            };

            let fears_broken = if let Some(mut active_auras) = active_auras_opt {
                // Break real Fears only. Death Coil's horror shares
                // AuraType::Fear but resolves to DRCategory::Horror — it
                // survives the break (TBC horror-bypasses-fear-immunity rule).
                let before = active_auras.auras.len();
                active_auras.auras.retain(|a| {
                    !(a.effect_type == AuraType::Fear
                        && a.dr_category() != Some(DRCategory::Horror))
                });
                let removed = before - active_auras.auras.len();
                active_auras.auras.push(immunity_aura);
                removed
            } else {
                // No auras yet — insert new ActiveAuras with FearImmunity
                // Note: .chain() auto-inserts ApplyDeferred, so this is visible to apply_pending_auras
                commands.entity(pending.caster).insert(ActiveAuras {
                    auras: vec![immunity_aura],
                });
                0
            };

            let caster_id = combatant_id(pending.caster_team, pending.caster_slot, pending.caster_class);

            // Log activation
            combat_log.log(
                CombatLogEventType::Buff,
                format!("{} uses Berserker Rage", caster_id),
            );

            // Log fear removal if any
            if fears_broken > 0 {
                combat_log.log(
                    CombatLogEventType::Buff,
                    format!(
                        "{}'s Berserker Rage breaks {} fear effect{}",
                        caster_id,
                        fears_broken,
                        if fears_broken > 1 { "s" } else { "" }
                    ),
                );
            }

            info!(
                "Team {} {} activates Berserker Rage (broke {} fear effects)",
                pending.caster_team,
                pending.caster_class.name(),
                fears_broken
            );

            // Activation visual: the TBC-style black angry mask + red glow at
            // the Warrior's head. Marker is spawned in both modes (like
            // ScreamBurst); the mesh/texture are attached only by the
            // graphical-only systems in states/mod.rs, so headless is unaffected.
            commands.spawn((
                BerserkMask {
                    caster: pending.caster,
                    lifetime: 1.4,
                    initial_lifetime: 1.4,
                },
                PlayMatchEntity,
            ));

            // Spawn white "Berserker Rage" FCT on the Warrior (status text stays
            // white per the color budget; the label carries the information)
            let text_position = transform.translation + Vec3::new(0.0, super::super::FCT_HEIGHT, 0.0);
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(pending.caster) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: "Berserker Rage".to_string(),
                    color: egui::Color32::WHITE,
                    lifetime: 2.0,
                    vertical_offset: offset_y,
                    is_crit: false,
                },
                PlayMatchEntity,
            ));
        }

        // Remove the pending entity
        commands.entity(pending_entity).despawn();
    }
}
