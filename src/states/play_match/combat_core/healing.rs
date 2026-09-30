//! The one place healing lands.
//!
//! Every effect that raises a combatant's health — a direct heal, a heal over
//! time tick, a bloom, Holy Shock, a drain or lifesteal, Devour Magic — goes
//! through [`apply_healing`]. The rules that apply to ALL healing received live
//! here and nowhere else, so a new heal site cannot forget one of them:
//!
//! 1. [`AuraType::HealingReduction`] (Mortal Strike's Mortal Wound and the
//!    other healing debuffs): Classic cuts all healing received, heal over time
//!    ticks included.
//! 2. The Mortal Wounds tell for that cut, bracketing the debuff alone.
//! 3. [`ArenaDampening`], after the debuff.
//! 4. The overheal clamp.
//!
//! A rule that stops healing outright (a healing immunity) belongs at the top
//! of [`apply_healing`], before the reduction, so it covers every site at once.
//!
//! `tests/heal_site_audit.rs` holds every health-raising write in `src/` to
//! this function: a write elsewhere, or a heal system that stops calling it,
//! fails `cargo test`. Absorb shields are not healing and do not come here
//! (only dampening applies to them, at `apply_pending_auras`).

use bevy::prelude::*;

use super::super::components::*;

/// Fraction of a heal that a [`AuraType::HealingReduction`] debuff refused, or
/// `None` when nothing was cut.
///
/// Decides whether [`apply_healing`] spawns a [`HealingRefused`] cosmetic
/// marker. Returns `None` for a zero or negative heal (nothing to refuse) and
/// for a reduction too small to see, so an unafflicted target never spawns a
/// marker and headless never accumulates them for ordinary healing. Pure — no
/// RNG, no side effects.
pub fn refused_fraction(before: f32, after: f32) -> Option<f32> {
    /// Below this the ash would be a handful of invisible motes.
    const MIN_VISIBLE_REFUSAL: f32 = 0.01;
    if before <= 0.0 {
        return None;
    }
    let refused = ((before - after) / before).clamp(0.0, 1.0);
    (refused > MIN_VISIBLE_REFUSAL).then_some(refused)
}

/// Land `amount` of healing on `target` and return the effective healing (the
/// health actually gained, overheal excluded) — the figure every site credits,
/// logs and shows.
///
/// `amount` is the heal as the effect computed it: stat scaling, crit and
/// stacks already in, no healing reduction or dampening yet. `target_auras`
/// are the TARGET's auras (the caster's own, for a drain or lifesteal).
///
/// Byte-neutral beyond the heal itself: no `game_rng` draw. The tell is
/// spawned in both modes and read only by the graphical
/// `rendering/effects/mortal_wounds.rs`.
pub fn apply_healing(
    commands: &mut Commands,
    target_entity: Entity,
    target: &mut Combatant,
    target_auras: Option<&ActiveAuras>,
    dampening: &ArenaDampening,
    amount: f32,
) -> f32 {
    // Healing reduction: each debuff's magnitude is a multiplier (0.65 = 35%
    // reduction), and two of them compound.
    let mut healing = amount;
    if let Some(auras) = target_auras {
        for aura in &auras.auras {
            if aura.effect_type == AuraType::HealingReduction {
                healing *= aura.magnitude;
            }
        }
    }

    // Mortal Wounds tell: the debuff has no body treatment, it states itself
    // by visibly breaking the heal. Before dampening, so the tell reflects the
    // debuff's cut only.
    if let Some(refused) = refused_fraction(amount, healing) {
        commands.spawn((
            HealingRefused {
                target: target_entity,
                refused_fraction: refused,
            },
            PlayMatchEntity,
        ));
    }

    // Arena dampening: time-ramped reduction of all healing.
    let healing = dampening.apply(healing);

    let effective = healing.min(target.max_health - target.current_health);
    target.current_health = (target.current_health + healing).min(target.max_health);
    effective
}
