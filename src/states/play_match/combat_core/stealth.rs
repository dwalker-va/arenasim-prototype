//! Stealth ending because something happened TO the unit.

use super::super::components::*;
use super::super::utils::combat_log_id_for;
use crate::combat::log::{CombatLog, CombatLogEventType};

/// Reveal `target` if it is stealthed, logging
/// `[STEALTH] <target> is revealed by <source>`. Returns whether it was.
///
/// This is the one place stealth ends because of something done TO the unit,
/// and exactly two things do it:
///
/// - **damage that reaches health** — [`super::apply_damage_with_absorb`], the
///   funnel every damage source passes through. Damage a shield fully absorbs,
///   or that Divine Shield blocks, never reaches health and reveals nothing;
/// - **a hostile aura that lands** — `auras::apply_pending_auras`, the funnel
///   every aura passes through, for any aura [`Aura::is_hostile_effect`] (the
///   classifier Divine Shield blocks on). The two zone/poison refreshers that
///   push a slow directly, bypassing the pending pipeline so it never
///   diminishes (the Frost Trap zone, Crippling Poison), call this too. An aura
///   that is blocked — immune, DR-immune, charging — never lands and reveals
///   nothing, the same rule as an absorbed hit.
///
/// A Rogue ending its OWN stealth — an opener, an auto-attack, a Kick — is not
/// a reveal and does not come here.
///
/// **When Vanish is added** it grants ~0.5-1.0s of immunity to incoming damage
/// and effects, INCLUDING projectiles already in flight and DoTs already
/// ticking — Classic's rule. That short immunity is the whole resolve-time
/// check: both funnels already refuse to reveal through an immunity, so no
/// per-projectile re-validation of the target's stealth is needed beyond it.
pub fn reveal_stealthed(target: &mut Combatant, source: &str, combat_log: &mut CombatLog) -> bool {
    if !target.stealthed {
        return false;
    }
    target.stealthed = false;
    // Only a Rogue stealths, and a pet is never one, so the id needs no pet.
    combat_log.log(
        CombatLogEventType::CrowdControl,
        format!(
            "[STEALTH] {} is revealed by {}",
            combat_log_id_for(target, None),
            source
        ),
    );
    true
}
