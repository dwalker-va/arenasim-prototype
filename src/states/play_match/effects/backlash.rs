//! Unstable Affliction Dispel Backlash Processing
//!
//! When an opposing-team combatant dispels Warlock's Unstable Affliction (UA),
//! the dispeller suffers two consequences:
//!   1. Direct Shadow damage equal to the snapshotted spell-power value stored on the aura.
//!   2. A 5-second Silence aura that prevents mana-cost casts.
//!
//! ## Why a separate system (not inline in `process_dispels`)
//!
//! `process_dispels` holds `&mut Combatant` for the dispel TARGET (the UA-bearing
//! combatant). The DISPELLER is a different entity, and mutating its `Combatant` to
//! apply backlash damage would conflict with that borrow. Instead, `process_dispels`
//! spawns a `BacklashPending` event component, and this dedicated system runs
//! immediately afterwards in the same `CombatSystemPhase::ResourcesAndAuras` to
//! consume those events with its own query of `&mut Combatant`.
//!
//! ## Damage-before-silence ordering invariant
//!
//! Damage is applied first. If the dispeller dies from the backlash, we skip
//! spawning the Silence aura — there is no point silencing a dead entity, and
//! attaching auras to dead entities would cause stale data in the ECS.
//!
//! ## Crit
//!
//! Backlash does NOT roll crit. The damage value snapshotted onto the aura at
//! cast time is final. We call `apply_damage_with_absorb` directly.

use bevy::prelude::*;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::abilities::SpellSchool;
use crate::states::play_match::combat_core::apply_damage_with_absorb;
use crate::states::play_match::components::*;

/// Pending backlash event spawned by `process_dispels` when an opposing-team
/// combatant strips an Unstable Affliction aura. Consumed by `process_backlash`
/// in the same Phase 1 tick.
#[derive(Component)]
pub struct BacklashPending {
    /// The entity that performed the dispel — receives damage and silence.
    pub dispeller: Entity,
    /// Snapshotted Shadow damage (already includes Warlock's spell-power scaling
    /// computed at UA cast time). Applied raw via `apply_damage_with_absorb`,
    /// no crit roll, no further scaling.
    pub damage: f32,
    /// Silence duration in seconds. Hardcoded to 5.0 by `process_dispels` for the
    /// MVP; future iterations can source this from the ability's
    /// `DispelBacklashConfig` if per-ability tuning becomes useful.
    pub silence_duration: f32,
    /// The original Warlock who applied the UA aura. Recorded as the Silence aura's
    /// `caster` so combat-log attribution and DR bookkeeping point at the right player.
    pub caster: Entity,
}

/// Apply UA dispel backlash: Shadow damage and Silence to the dispeller.
///
/// Runs in `CombatSystemPhase::ResourcesAndAuras` AFTER `process_dispels` so the
/// `BacklashPending` events spawned this frame are consumed in the same tick
/// (no one-frame delay between dispel and backlash).
///
/// The Silence aura is spawned via `AuraPending`, so it flows through the standard
/// `apply_pending_auras` pipeline and picks up DR automatically — do NOT apply DR
/// manually here. Note: `apply_pending_auras` already ran earlier in this same
/// chain, so the Silence will land on the next frame's Phase 1, which is the
/// expected behavior.
pub fn process_backlash(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    pending: Query<(Entity, &BacklashPending)>,
    mut combatants: Query<(&mut Combatant, Option<&mut ActiveAuras>)>,
) {
    for (pending_entity, event) in pending.iter() {
        // Despawn the event regardless of outcome.
        commands.entity(pending_entity).despawn();

        // Look up the dispeller. If they were already despawned (race on death this
        // same frame from another source), skip silently.
        let Ok((mut dispeller, dispeller_auras)) = combatants.get_mut(event.dispeller) else {
            continue;
        };

        if !dispeller.is_alive() {
            // Dispeller already dead from another source this frame — no point
            // applying damage or silence. Skip.
            continue;
        }

        // ----- Step 1: Apply backlash damage -----
        let (actual_damage, absorbed, dispeller_team, dispeller_class_name, still_alive) = {
            let (actual_damage, absorbed) = apply_damage_with_absorb(
                event.damage,
                &mut dispeller,
                dispeller_auras.map(|a| a.into_inner()),
                SpellSchool::Shadow,
            );
            (
                actual_damage,
                absorbed,
                dispeller.team,
                dispeller.class.name(),
                dispeller.is_alive(),
            )
        };
        // Dispeller borrow ends here so we can credit the caster (different entity)
        // without aliasing on the `combatants` query.
        // Credit the caster's damage_dealt the same way casting.rs:344-350 and
        // auto_attack.rs:283 do: actual_damage + absorbed. Damage that hit absorbs
        // still counts as "dealt" — only the target's damage_taken intentionally
        // omits absorbed amounts (see apply_damage_with_absorb).
        if let Ok((mut caster_combatant, _)) = combatants.get_mut(event.caster) {
            caster_combatant.damage_dealt += actual_damage + absorbed;
        }

        combat_log.log(
            CombatLogEventType::Damage,
            format!(
                "[BACKLASH] Team {} {} takes {:.0} Shadow damage and is Silenced by Unstable Affliction",
                dispeller_team, dispeller_class_name, actual_damage
            ),
        );

        info!(
            "[BACKLASH] Team {} {} takes {:.0} Shadow damage from Unstable Affliction",
            dispeller_team, dispeller_class_name, actual_damage
        );

        // Spawn the BacklashBurst visual at the dispeller (graphical mode only —
        // the spawn/update/cleanup systems live in rendering/effects.rs and are
        // registered exclusively in src/states/mod.rs).
        commands.spawn((
            BacklashBurst {
                target: event.dispeller,
                lifetime: 0.3,
                initial_lifetime: 0.3,
            },
            PlayMatchEntity,
        ));

        // ----- Step 2: Apply Silence aura (only if dispeller survived) -----
        // This is the damage-before-silence invariant — we never attach a Silence
        // aura to a dead entity.
        if !still_alive {
            continue;
        }

        let silence_aura = Aura {
            effect_type: AuraType::Silence,
            duration: event.silence_duration,
            magnitude: 1.0,
            caster: Some(event.caster),
            ability_name: "Unstable Affliction".to_string(),
            spell_school: Some(SpellSchool::Shadow),
            break_on_damage_threshold: -1.0, // Silence does not break on damage
            ..Default::default()
        };

        commands.spawn(AuraPending {
            target: event.dispeller,
            aura: silence_aura,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: `BacklashPending` is a valid Component and its public fields can
    /// be constructed. Real integration testing (UA -> dispel -> backlash damage +
    /// silence -> AI behaviour) waits for Unit 6 (Warlock AI casts UA) and Unit 8
    /// (sim-driven balance tuning).
    #[test]
    fn backlash_pending_constructs() {
        let mut world = World::new();
        let dispeller = world.spawn_empty().id();
        let caster = world.spawn_empty().id();

        let pending = BacklashPending {
            dispeller,
            damage: 123.0,
            silence_duration: 5.0,
            caster,
        };

        // Field access — proves the struct shape this module promises to other units.
        assert_eq!(pending.dispeller, dispeller);
        assert_eq!(pending.caster, caster);
        assert!((pending.damage - 123.0).abs() < f32::EPSILON);
        assert!((pending.silence_duration - 5.0).abs() < f32::EPSILON);

        // Compile-time check that BacklashPending implements Component:
        // spawn() requires the `Bundle` trait, which `Component` blanket-impls.
        let _entity = world.spawn(pending).id();
    }
}
