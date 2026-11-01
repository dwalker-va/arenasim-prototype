//! On-use item resolution.
//!
//! The AI decides a use (`class_ai::on_use`) and spawns an [`OnUsePending`];
//! this system carries it out, one arm per [`OnUseEffect`]. The deferred
//! pattern Divine Shield and Berserker Rage use, for the same reason: the AI
//! holds no mutable auras. Runs beside them, BEFORE `apply_pending_auras`, so
//! a CC queued for the frame the wearer breaks free lands after the break,
//! as TBC's medallion grants no immunity.

use bevy::prelude::*;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::components::*;
use crate::states::play_match::on_use_items::{cc_break_breaks, OnUseEffect, OnUsePending};
use crate::states::play_match::utils::combatant_id;

/// Resolve every pending on-use.
pub fn process_on_use_items(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    pending_uses: Query<(Entity, &OnUsePending)>,
    mut combatants: Query<(&Combatant, Option<&mut ActiveAuras>)>,
) {
    for (pending_entity, pending) in pending_uses.iter() {
        commands.entity(pending_entity).despawn();
        let Ok((combatant, active_auras)) = combatants.get_mut(pending.wearer) else {
            continue;
        };
        if !combatant.is_alive() {
            continue;
        }
        let wearer_id = combatant_id(combatant.team, combatant.slot, combatant.class);
        combat_log.log(
            CombatLogEventType::Buff,
            format!("[ITEM] {} uses {}", wearer_id, pending.item_name),
        );
        match pending.effect {
            OnUseEffect::CcBreak => {
                let broken = active_auras
                    .map(|mut auras| auras.break_debuffs_where(cc_break_breaks))
                    .unwrap_or_default();
                if !broken.is_empty() {
                    combat_log.log(
                        CombatLogEventType::Buff,
                        format!(
                            "[ITEM] {}'s {} breaks {}",
                            wearer_id,
                            pending.item_name,
                            broken.join(", ")
                        ),
                    );
                }
            }
        }
    }
}
