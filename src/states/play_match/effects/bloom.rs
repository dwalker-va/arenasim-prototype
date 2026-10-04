//! Bloom Processing
//!
//! A BLOOMING aura lands a direct heal on its bearer when it ENDS — by running
//! out, or by being dispelled or purged (TBC Lifebloom). See
//! [`Aura::bloom_heal`].
//!
//! ## Why one system for both endings
//!
//! The two endings are noticed by two different systems: `process_hot_ticks`
//! sees an aura's last frame, and `process_dispels` sees a removal. Each spawns
//! a [`BloomPending`] and this system lands every one of them, so there is ONE
//! heal site for a bloom — one place for the log line and the floating text,
//! landing through `apply_healing` like every heal — rather than two copies
//! that could disagree. Same shape as `BacklashPending` / `process_backlash`.
//!
//! ## What does not bloom
//!
//! A refresh (the aura has not ended), and the bearer dying (nothing to heal).

use bevy::prelude::*;
use bevy_egui::egui;

use crate::combat::log::CombatLog;
use crate::states::play_match::combat_core::apply_healing;
use crate::states::play_match::components::*;
use crate::states::play_match::match_config::CharacterClass;
use crate::states::play_match::utils::{combat_log_id_for, combatant_id, get_next_fct_offset};

/// A bloom waiting to land: the heal an aura owes its bearer for ending.
#[derive(Component)]
pub struct BloomPending {
    /// The bearer, who is healed.
    pub target: Entity,
    /// The heal before healing reduction and dampening — already scaled by
    /// the stacks the aura ended at ([`Aura::bloom_heal`]).
    pub amount: f32,
    /// The applying ability's name, for the log.
    pub ability_name: String,
    /// Who cast the aura, for healing credit and the log. `None` when the
    /// caster is gone; the bearer is still healed.
    pub caster: Option<Entity>,
}

impl BloomPending {
    /// The bloom `aura` owes `bearer` for ending, or `None` when it does not
    /// bloom.
    pub fn for_ending(bearer: Entity, aura: &Aura) -> Option<Self> {
        aura.bloom_heal().map(|amount| Self {
            target: bearer,
            amount,
            ability_name: aura.ability_name.clone(),
            caster: aura.caster,
        })
    }
}

/// Land every pending bloom on its bearer.
pub fn process_blooms(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    dampening: Res<ArenaDampening>,
    pending_blooms: Query<(Entity, &BloomPending)>,
    mut combatants: Query<(&mut Combatant, &Transform, Option<&ActiveAuras>)>,
    pet_query: Query<&Pet>,
    mut fct_states: Query<&mut FloatingTextState>,
) {
    for (pending_entity, pending) in pending_blooms.iter() {
        commands.entity(pending_entity).despawn();

        // The caster's identity for the log, read before the bearer is
        // borrowed mutably (a self-cast bloom is the same entity).
        let caster: Option<(u8, u8, CharacterClass)> = pending
            .caster
            .and_then(|c| combatants.get(c).ok())
            .map(|(c, _, _)| (c.team, c.slot, c.class));

        let Ok((mut target, target_transform, target_auras)) = combatants.get_mut(pending.target)
        else {
            continue;
        };
        if !target.is_alive() {
            continue;
        }
        let target_position = target_transform.translation;

        let actual = apply_healing(
            &mut commands,
            pending.target,
            &mut target,
            target_auras,
            &dampening,
            pending.amount,
        );

        // The burst a bloom plays, spawned here — after the alive check, at the
        // one site every bloom lands — so it blooms exactly when the heal does.
        // Purely cosmetic, like the heal landings; rendered only in graphical
        // mode (`rendering/effects/druid_heals.rs`).
        if let Some(kind) = BloomVisual::for_bloom(&pending.ability_name) {
            commands.spawn((
                BloomBurst {
                    target: pending.target,
                    kind,
                },
                PlayMatchEntity,
            ));
        }

        let target_id = combat_log_id_for(&target, pet_query.get(pending.target).ok());
        let self_bloom = pending.caster == Some(pending.target);
        if self_bloom {
            target.healing_done += actual;
        }
        let text_position = target_position + Vec3::new(0.0, super::super::FCT_HEIGHT, 0.0);
        let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(pending.target) {
            get_next_fct_offset(&mut fct_state)
        } else {
            (0.0, 0.0)
        };
        commands.spawn((
            FloatingCombatText {
                world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                text: format!("+{:.0}", actual),
                color: egui::Color32::from_rgb(100, 255, 100), // Green for healing
                lifetime: 1.5,
                vertical_offset: offset_y,
                is_crit: false, // a bloom never crits
            },
            PlayMatchEntity,
        ));

        // Healing credit goes to the caster, as for a tick.
        let caster_id = match caster {
            Some((team, slot, class)) => combatant_id(team, slot, class),
            None => target_id.clone(),
        };
        if let Some(caster_entity) = pending.caster.filter(|_| !self_bloom) {
            if let Ok((mut caster_combatant, _, _)) = combatants.get_mut(caster_entity) {
                caster_combatant.healing_done += actual;
            }
        }

        let message = format!(
            "{}'s {} blooms on {} for {:.0}",
            caster_id, pending.ability_name, target_id, actual
        );
        combat_log.log_healing(
            caster_id,
            target_id,
            pending.ability_name.clone(),
            actual,
            false,
            message,
        );
    }
}
