//! Shared pre-cast guard helper for class AI `try_*` ability functions.
//!
//! Almost every ability decision repeats the same opening guard sequence:
//! friendly-CC check, spell-school lockout, silence, range/cooldown/mana.
//! `pre_cast_ok` runs that sequence in one place so individual `try_*`
//! functions can collapse the preamble to a single `if !pre_cast_ok(...)`
//! line.
//!
//! Each guard that varies between abilities is opt-in via [`PreCastOpts`].

use bevy::prelude::*;

use super::CombatContext;
use crate::states::play_match::abilities::{is_silenced, is_spell_school_locked, AbilityType};
use crate::states::play_match::ability_config::AbilityConfig;
use crate::states::play_match::components::{ActiveAuras, Combatant};

/// Opt-in guards layered on top of the universal pre-cast checks.
///
/// The universal checks (always applied) are: spell-school lockout, silence
/// (gated on `mana_cost > 0` and caster's resource type via [`is_silenced`]),
/// per-ability cooldown, and `can_cast_config` (mana / range / min-range /
/// stealth) for targeted casts or a bare mana check for self-targeted casts.
#[derive(Debug, Clone, Copy, Default)]
pub struct PreCastOpts {
    /// Skip the cast if the target is currently CC'd by an aura on our team
    /// that breaks on any damage (Polymorph, Freezing Trap, Sap, ...).
    /// Prevents the AI from breaking its own team's CC. (BUG-1.)
    pub check_friendly_cc: bool,

    /// Skip the cast if the target carries a DoT applied by our team. Used
    /// by Polymorph (and analogous incapacitates) so we don't blow up our
    /// own CC the moment a friendly DoT ticks. (BUG-2.)
    pub check_friendly_dots: bool,

    /// Skip the cast if the target is currently damage-immune (Divine
    /// Shield). Mind Blast, Holy Shock damage, etc.
    pub check_target_immune: bool,

    /// Allow the cast even if the caster is silenced. Reserved for
    /// abilities that explicitly bypass silence (Divine Shield).
    pub bypass_silence: bool,
}

/// Run the standard pre-cast guard sequence.
///
/// Returns `true` if every check passes and the caller should proceed with
/// the success-side bookkeeping (mana deduction, GCD, log, spawning auras
/// or pending components, etc.). Returns `false` on the first failed
/// guard.
///
/// `target = Some((entity, position))` enables friendly-CC / friendly-DoT /
/// target-immunity checks (when opt-in) and routes the final mana/range
/// check through [`AbilityType::can_cast_config`]. `target = None` means
/// the cast is self-targeted, so the final check is a bare `current_mana
/// >= mana_cost`.
///
/// Guards run in this order so that cheap checks fail fast:
/// 1. friendly-CC (opt-in)
/// 2. friendly-DoTs (opt-in)
/// 3. target damage immunity (opt-in)
/// 4. spell-school lockout
/// 5. silence (skipped when `bypass_silence`; otherwise auto-gated on
///    `mana_cost > 0` and caster resource type)
/// 6. per-ability cooldown
/// 7. mana / range / min-range / stealth (via `can_cast_config` for
///    targeted casts; mana-only for self-targeted)
pub fn pre_cast_ok(
    ability: AbilityType,
    def: &AbilityConfig,
    caster: &Combatant,
    caster_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target: Option<(Entity, Vec3)>,
    ctx: &CombatContext,
    opts: PreCastOpts,
) -> bool {
    if let Some((target_entity, _)) = target {
        if opts.check_friendly_cc && ctx.has_friendly_breakable_cc(target_entity) {
            return false;
        }
        if opts.check_friendly_dots && ctx.has_friendly_dots_on_target(target_entity) {
            return false;
        }
        if opts.check_target_immune && ctx.entity_is_immune(target_entity) {
            return false;
        }
    }

    if is_spell_school_locked(def.spell_school, auras) {
        return false;
    }

    if !opts.bypass_silence && def.mana_cost > 0.0 && is_silenced(caster, auras) {
        return false;
    }

    if caster.ability_cooldowns.contains_key(&ability) {
        return false;
    }

    match target {
        Some((_, target_pos)) => ability.can_cast_config(caster, target_pos, caster_pos, def),
        // Self-targeted / no-target casts intentionally skip range. Callers like
        // Frost Nova or Paladin Aura enforce range themselves during target
        // selection; for genuine self-buffs there is no range to check.
        None => caster.current_mana >= def.mana_cost,
    }
}
