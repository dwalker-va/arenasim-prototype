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
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::components::{ActiveAuras, AuraType, Combatant};
use crate::states::play_match::map_geometry::has_line_of_sight;

/// Opt-in guards layered on top of the universal pre-cast checks.
///
/// The universal checks (always applied) are: spell-school lockout, silence
/// (for an ability a silence stops, [`AbilityConfig::silence_stops`], on a
/// caster [`is_silenced`]),
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

/// The rejection note for a cast refused because the caster is in Travel Form.
pub const SHAPESHIFTED_NOTE: &str = "shapeshifted: nothing is cast in Travel Form";

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
/// 0. a cast that applies a hold ([`applies_hold`]) on a target already held
///    ([`CombatContext::held_in_hard_cc`]) — always, not opt-in: no hard CC is
///    stacked on a unit its team already holds. Under the Chain play a
///    hand-off onto the lock's subject is judged where it lands
///    ([`hold_lands_after`])
/// 1. friendly-CC (opt-in)
/// 2. friendly-DoTs (opt-in)
/// 3. target damage immunity (opt-in)
/// 4. shapeshift: nothing is cast in Travel Form
/// 5. spell-school lockout
/// 6. silence (skipped when `bypass_silence`; otherwise only for an ability
///    a silence stops, [`AbilityConfig::silence_stops`] — so a silenced Hunter
///    still shoots and a silenced Druid still shifts — on a silenced mana user)
/// 7. per-ability cooldown
/// 8. mana / range / min-range / stealth (via `can_cast_config` for
///    targeted casts; mana-only for self-targeted)
/// 9. line-of-sight from caster to target (targeted casts only; skipped for
///    self-targeted / ground-placed casts). Runs AFTER `can_cast_config` so
///    an out-of-range target fails on range, not LoS. A no-op on maps with no
///    obstacles (empty `ctx.obstacles`).
/// 10. the Burst play's fit ([`fits_window`]): under it, a cast on the kill
///     window's subject that would land after the window's `until` is not
///     started. A no-op outside the play (every `Legacy` decision).
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
        if applies_hold(def)
            && ctx
                .held_in_hard_cc_at(
                    target_entity,
                    super::Incoming::of(def),
                    hold_lands_after(def, caster_pos, auras, target, ctx),
                )
                .is_some()
        {
            return false;
        }
    }
    pre_cast_ok_but_hold(ability, def, caster, caster_pos, auras, target, ctx, opts)
}

/// [`pre_cast_ok`] without its step 0, the hold rule: every other gate, in
/// the same order. The Chain play asks it of a hand-off that is not due yet
/// (`class_ai::chain`) — the lock it will chain onto is what refuses it now,
/// and the question is whether everything else would let it go when it is.
#[allow(clippy::too_many_arguments)]
pub fn pre_cast_ok_but_hold(
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

    if !own_state_ok(ability, def, caster, auras, opts) {
        return false;
    }

    match target {
        Some((_, target_pos)) => {
            if !ability.can_cast_config(caster, target_pos, caster_pos, def) {
                return false;
            }
            // Line-of-sight gate — a DISTINCT predicate placed immediately
            // after can_cast_config (so range wins precedence). Segment runs
            // from the caster's origin to the target's origin (both ~center of
            // mass at y≈1.0). Empty obstacle lists → always clear (no-op).
            has_line_of_sight(ctx.obstacles, caster_pos, target_pos)
                && fits_window(def, caster_pos, auras, target, ctx).is_ok()
        }
        // Self-targeted / no-target casts intentionally skip range AND
        // line-of-sight. Callers like Frost Nova or Paladin Aura enforce range
        // themselves during target selection; for genuine self-buffs there is
        // no target segment to test.
        None => caster.current_mana >= def.mana_cost,
    }
}

/// When a cast of `def`, begun now from `caster_pos` at a target standing at
/// `target_pos`, first lands: its cast time (as `calculate_cast_time` slows
/// it), its flight at its `projectile_speed`, and — for a cast whose only
/// damage is over time (Rend, Corruption, a curse, Serpent Sting) — its first
/// tick. A cast that deals nothing lands when it arrives.
pub fn lands_after(
    def: &AbilityConfig,
    caster_pos: Vec3,
    target_pos: Vec3,
    auras: Option<&ActiveAuras>,
) -> f32 {
    let cast = calculate_cast_time(def.cast_time, auras);
    let flight = def
        .projectile_speed
        .filter(|speed| *speed > 0.0)
        .map_or(0.0, |speed| caster_pos.distance(target_pos) / speed);
    let first_tick = def
        .applies_aura
        .as_ref()
        .filter(|a| a.aura_type == AuraType::DamageOverTime && def.damage_base_max <= 0.0)
        .map_or(0.0, |a| a.tick_interval);
    cast + flight + first_tick
}

/// The Burst play's until-aware fit: under the play
/// ([`CombatContext::burst_window`]), a cast on the kill window's subject must
/// land ([`lands_after`]) no later than the window's `until`. `Err((lands_at,
/// until))` when it would not. Always `Ok` for any other cast: a target other
/// than the subject, a window with no known `until`, or no Burst play at all
/// — so it changes nothing outside the play.
pub fn fits_window(
    def: &AbilityConfig,
    caster_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target: Option<(Entity, Vec3)>,
    ctx: &CombatContext,
) -> Result<(), (f32, f32)> {
    let Some((entity, target_pos)) = target else {
        return Ok(());
    };
    let Some(window) = ctx.burst_window().filter(|w| w.subject == entity) else {
        return Ok(());
    };
    let (Some(until), Some(now)) = (window.until, ctx.situation_now()) else {
        return Ok(());
    };
    let lands_at = now + lands_after(def, caster_pos, target_pos, auras);
    if lands_at <= until {
        Ok(())
    } else {
        Err((lands_at, until))
    }
}

/// How far ahead the hold rule looks for a cast of `def` on `target`
/// ([`super::CombatContext::held_in_hard_cc_at`]): under the Chain play, a
/// hand-off onto the subject of the lock being handed off is judged at the
/// moment it LANDS ([`lands_after`]) — a 1.5s Polymorph begun to land in the
/// lock's last moments starts while more than the chain window is left.
/// Every other cast is judged now (0), as the hold rule always has: so this
/// changes nothing outside the play.
pub fn hold_lands_after(
    def: &AbilityConfig,
    caster_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target: Option<(Entity, Vec3)>,
    ctx: &CombatContext,
) -> f32 {
    match target {
        Some((entity, target_pos)) if ctx.chain_window().is_some_and(|w| w.subject == entity) => {
            lands_after(def, caster_pos, target_pos, auras)
        }
        _ => 0.0,
    }
}

/// Does this ability HOLD what it lands on — apply crowd control that stops
/// its target acting ([`holds`](super::holds))? Read from the ability's own
/// `applies_aura`, so a new hard-CC ability is gated by [`pre_cast_ok`] with no
/// change here. A ground-placed cast (a trap) has no target for the gate to
/// ask about, so its placement asks this of the enemy it is aimed at.
pub fn applies_hold(def: &AbilityConfig) -> bool {
    def.applies_aura
        .as_ref()
        .is_some_and(|a| super::holds(a.aura_type))
}

/// The caster's own gates, steps 4-7 of [`pre_cast_ok`]: Travel Form, the
/// school lockout, silence (unless `opts.bypass_silence`) and the ability's
/// cooldown. Needs no context.
pub fn own_state_ok(
    ability: AbilityType,
    def: &AbilityConfig,
    caster: &Combatant,
    auras: Option<&ActiveAuras>,
    opts: PreCastOpts,
) -> bool {
    // Nothing is cast in Travel Form: the Druid shifts back first.
    !auras.is_some_and(ActiveAuras::is_shapeshifted)
        && !is_spell_school_locked(def.spell_school, auras)
        && (opts.bypass_silence || !def.silence_stops() || !is_silenced(caster, auras))
        && !caster.ability_cooldowns.contains_key(&ability)
}

/// Proof that one ability passed [`pre_cast_ok`] for the caster about to cast
/// it, and the only way to record that ability as chosen:
/// [`DecisionEventBuilder::choose`] takes one, and only [`cast_gate`] makes
/// one. A `try_*` that skips the gate has nothing to hand `choose`, so it does
/// not compile — and one that never calls `choose` leaves its ability unchosen
/// everywhere, which `tests/decision_properties.rs`
/// (`every_kit_ability_is_chosen_or_named`) fails on.
///
/// Not `Clone`: a clearance is spent on the one choice it was issued for, and
/// it carries its ability so the choice cannot name another.
///
/// It proves the gate RAN, not that it was handed the right caster. Pass the
/// caster's own `Combatant` and its own `ActiveAuras` — the silence, the
/// school lockout and Travel Form are read from them.
///
/// A new ability's `try_*` therefore needs nothing extra: it asks
/// [`cast_gate`] where it would have asked `pre_cast_ok`, rejects with the
/// `Err` reason, and spends the `Ok` clearance on its `choose`. So does a cast
/// decided outside the class dispatch: the interrupts
/// (`class_ai::interrupts`).
///
/// The stop site, pinned. A scene with a decision open compiles:
///
/// ```
/// use arenasim::states::match_config::CharacterClass;
/// use arenasim::states::play_match::abilities::AbilityType;
/// use arenasim::states::play_match::decision_trace::{ActorView, DecisionTrace, RejectionReason};
/// use bevy::prelude::{Entity, Vec3};
///
/// let mut trace = DecisionTrace::default();
/// let actor = ActorView::from_raw(
///     Entity::from_raw(1), 1, 0, CharacterClass::Mage, 1.0, 1.0, Vec3::ZERO,
/// );
/// let mut builder = trace.start_ability_decision(actor, None);
/// builder.reject(AbilityType::IceBarrier, RejectionReason::AlreadyApplied);
/// builder.finish();
/// ```
///
/// ...but choosing in it without a clearance does not:
///
/// ```compile_fail,E0308
/// use arenasim::states::match_config::CharacterClass;
/// use arenasim::states::play_match::abilities::AbilityType;
/// use arenasim::states::play_match::decision_trace::{ActorView, DecisionTrace, RejectionReason};
/// use bevy::prelude::{Entity, Vec3};
///
/// let mut trace = DecisionTrace::default();
/// let actor = ActorView::from_raw(
///     Entity::from_raw(1), 1, 0, CharacterClass::Mage, 1.0, 1.0, Vec3::ZERO,
/// );
/// let mut builder = trace.start_ability_decision(actor, None);
/// builder.choose(AbilityType::IceBarrier, None, true);
/// builder.finish();
/// ```
///
/// ...and nor does forging one:
///
/// ```compile_fail,E0451
/// use arenasim::states::play_match::abilities::AbilityType;
/// use arenasim::states::play_match::class_ai::cast_guard::Cleared;
///
/// let _forged = Cleared { ability: AbilityType::IceBarrier };
/// ```
///
/// [`DecisionEventBuilder::choose`]:
///     crate::states::play_match::decision_trace::DecisionEventBuilder::choose
#[must_use = "a clearance is spent by `DecisionEventBuilder::choose`"]
#[derive(Debug)]
pub struct Cleared {
    ability: AbilityType,
}

impl Cleared {
    /// The ability this clearance was issued for.
    pub fn ability(&self) -> AbilityType {
        self.ability
    }
}

/// The cast gate: [`pre_cast_ok`], with its failure classified for the trace
/// ([`classify_pre_cast_failure`]) and its success as a [`Cleared`] — the
/// token a choice needs.
///
/// `target = None` gates the caster alone (Travel Form, school lockout,
/// silence, cooldown, mana) and leaves range and sight to the caller, as
/// `pre_cast_ok` does for a self-cast or a ground-placed one.
#[allow(clippy::too_many_arguments)]
pub fn cast_gate(
    ability: AbilityType,
    def: &AbilityConfig,
    caster: &Combatant,
    caster_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target: Option<(Entity, Vec3)>,
    ctx: &CombatContext,
    opts: PreCastOpts,
) -> Result<Cleared, crate::states::play_match::decision_trace::RejectionReason> {
    if pre_cast_ok(ability, def, caster, caster_pos, auras, target, ctx, opts) {
        Ok(Cleared { ability })
    } else {
        Err(classify_pre_cast_failure(
            ability, def, caster, caster_pos, auras, target, ctx, opts,
        ))
    }
}

/// Decompose a `pre_cast_ok` failure into the specific `RejectionReason` that
/// caused it. Re-runs the same predicate sequence pre_cast_ok uses (in the same
/// order) so the emitted reason matches the gate that fired.
///
/// Used by every class AI's traced wrapper around `pre_cast_ok` — when the
/// guard fails, this turns the bool into a typed reason for the decision-trace
/// builder.
#[allow(clippy::too_many_arguments)]
pub fn classify_pre_cast_failure(
    ability: AbilityType,
    def: &AbilityConfig,
    caster: &Combatant,
    caster_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target: Option<(Entity, Vec3)>,
    ctx: &CombatContext,
    opts: PreCastOpts,
) -> crate::states::play_match::decision_trace::RejectionReason {
    use crate::states::play_match::abilities::{is_silenced, is_spell_school_locked};
    use crate::states::play_match::decision_trace::RejectionReason;

    if let Some((target_entity, _)) = target {
        if let Some(cc_type) = ctx
            .held_in_hard_cc_at(
                target_entity,
                super::Incoming::of(def),
                hold_lands_after(def, caster_pos, auras, target, ctx),
            )
            .filter(|_| applies_hold(def))
        {
            return RejectionReason::TargetAlreadyCCd { cc_type };
        }
        if opts.check_friendly_cc && ctx.has_friendly_breakable_cc(target_entity) {
            return RejectionReason::FriendlyBreakableCC;
        }
        if opts.check_friendly_dots && ctx.has_friendly_dots_on_target(target_entity) {
            return RejectionReason::FriendlyBreakableCC;
        }
        if opts.check_target_immune && ctx.entity_is_immune(target_entity) {
            return RejectionReason::TargetImmune;
        }
    }
    if auras.is_some_and(ActiveAuras::is_shapeshifted) {
        return RejectionReason::PreconditionUnmet {
            note: SHAPESHIFTED_NOTE.to_string(),
        };
    }
    if is_spell_school_locked(def.spell_school, auras) {
        return RejectionReason::SilencedOrLocked {
            school: def.spell_school,
        };
    }
    if !opts.bypass_silence && def.silence_stops() && is_silenced(caster, auras) {
        return RejectionReason::SilencedOrLocked {
            school: def.spell_school,
        };
    }
    if let Some(remaining) = caster.ability_cooldowns.get(&ability) {
        return RejectionReason::OnCooldown {
            remaining: *remaining,
        };
    }
    let resource_shortage = || -> RejectionReason {
        use crate::states::match_config::CharacterClass;
        use crate::states::play_match::decision_trace::ResourceKind;
        match caster.class {
            CharacterClass::Warrior => RejectionReason::InsufficientResource {
                resource: ResourceKind::Rage,
                have: caster.current_mana,
                need: def.mana_cost,
            },
            CharacterClass::Rogue => RejectionReason::InsufficientResource {
                resource: ResourceKind::Energy,
                have: caster.current_mana,
                need: def.mana_cost,
            },
            CharacterClass::Mage
            | CharacterClass::Priest
            | CharacterClass::Warlock
            | CharacterClass::Paladin
            | CharacterClass::Hunter
            | CharacterClass::Shaman
            | CharacterClass::Druid => RejectionReason::InsufficientMana {
                have: caster.current_mana,
                need: def.mana_cost,
            },
        }
    };

    // Predicate order MUST match `AbilityType::can_cast_config` exactly:
    // mana first, then range, then min_range, then stealth. Otherwise the
    // trace can lie about why a cast failed (e.g., reporting OutOfRange
    // when can_cast_config actually short-circuited on mana).
    match target {
        Some((_, target_pos)) => {
            if caster.current_mana < def.mana_cost {
                return resource_shortage();
            }
            let distance = caster_pos.distance(target_pos);
            if distance > def.range {
                return RejectionReason::OutOfRange {
                    distance,
                    max: def.range,
                };
            }
            if let Some(min_range) = def.min_range {
                if distance < min_range {
                    return RejectionReason::WithinDeadZone {
                        distance,
                        min: min_range,
                    };
                }
            }
            if matches!(ability, AbilityType::Ambush | AbilityType::CheapShot) && !caster.stealthed
            {
                return RejectionReason::PreconditionUnmet {
                    note: "stealth required (Ambush/CheapShot)".into(),
                };
            }
            // Line-of-sight — the same relative position as in `pre_cast_ok`
            // (after can_cast_config's mana/range/min_range/stealth, mirrored
            // here as the inlined checks above). Keep this AFTER the stealth
            // check so the predicate order stays lockstep with pre_cast_ok.
            if !has_line_of_sight(ctx.obstacles, caster_pos, target_pos) {
                return RejectionReason::LosBlocked;
            }
            if let Err((lands_at, until)) = fits_window(def, caster_pos, auras, target, ctx) {
                return RejectionReason::OutlastsWindow { lands_at, until };
            }
        }
        None => {
            if caster.current_mana < def.mana_cost {
                return resource_shortage();
            }
        }
    }
    RejectionReason::PreconditionUnmet {
        note: "can_cast_config failed".into(),
    }
}

/// The reason to trace when a reach-first chooser
/// ([`super::cast_reach`] / [`super::sight_blocks`]) found no candidate in
/// reach. The chooser runs before `pre_cast_ok`, so its `LosBlocked` verdict
/// was reached without asking about the caster. `pre_cast_ok` puts the
/// caster's own state ahead of sight, so a caster that could not have cast
/// anyway is reported for that reason: Travel Form, a school lockout, a
/// silence, a cooldown or a lack of mana. Every other `reason` passes through
/// untouched. On an obstacle-free map no chooser says `LosBlocked`, so this
/// changes nothing there.
#[allow(clippy::too_many_arguments)]
pub fn unreached_reason(
    reason: crate::states::play_match::decision_trace::RejectionReason,
    ability: AbilityType,
    def: &AbilityConfig,
    caster: &Combatant,
    caster_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    opts: PreCastOpts,
) -> crate::states::play_match::decision_trace::RejectionReason {
    use crate::states::play_match::decision_trace::RejectionReason;
    if matches!(reason, RejectionReason::LosBlocked)
        && !pre_cast_ok(ability, def, caster, caster_pos, auras, None, ctx, opts)
    {
        classify_pre_cast_failure(ability, def, caster, caster_pos, auras, None, ctx, opts)
    } else {
        reason
    }
}
