//! The Chain play's shared half (`situation::Play::Chain`).
//!
//! While the team holds an enemy locked and the lock ends within
//! `situation::CHAIN_HORIZON_SECS` (`TeamSituation::lock_ending`: of the
//! enemies the team holds in a stun, a fear or horror, a polymorph, an
//! incapacitate or a cyclone, the one whose hold ends SOONEST — the healer on
//! a tie), a unit on a team that runs the play — `AiProfile::TeamPlan`,
//! unless the play is withheld for measurement, and not a class
//! `situation::CHAIN_EXCEPTIONS` names — puts its CHAIN step at the head of
//! its chain, below a peel step and above a burst step: a HAND-OFF of its own
//! crowd control ([`chain_tools`]) onto that enemy, timed to land in the
//! lock's last moments, so the enemy never gets a free global cooldown
//! between the two. Crowd control compounds: a healer held without a gap
//! cannot heal the damage that lands on its team at all, and a kill target
//! held without one cannot answer it (`cc-compounds-not-adds`).
//!
//! The lock may be on any enemy, not only the healer: in this sim a team
//! rarely holds the enemy healer with a lock its other member can extend
//! (one member usually made the lock, with the tool that would extend it),
//! while a stun on the kill target is extended by any teammate's stun.
//!
//! The play is a reordering plus a timing, not a scorer (the architecture's
//! §4.2–4.3: "hold a stun to chain into the lock's end" is a fact about time,
//! not a value):
//!
//! - **When it lands.** A hand-off lands when [`super::cast_guard::lands_after`]
//!   says — its cast time (slowed by Curse of Tongues) and its flight — the
//!   same model the Burst play's fit uses. It is DUE when it would land no
//!   earlier than [`HANDOFF_LEAD`] before the lock's `until`, and no later
//!   than the `until`. Earlier spends the new lock alongside the old one;
//!   later leaves the enemy free in between. Either is traced
//!   `MissesHandoff { lands_at, until }`.
//! - **The hold rule, at landing.** The cast gate refuses a hard CC on a unit
//!   still held for longer than the chain window. Under the play a hand-off
//!   onto the lock's subject is judged where it LANDS
//!   (`cast_guard::hold_lands_after`), so a 1.5s Polymorph begun to land in
//!   the lock's last moments is not refused at its start.
//! - **Holding the global cooldown.** A hand-off that is not due yet, but
//!   whose moment comes within one global cooldown and that every other gate
//!   would let go (`cast_guard::pre_cast_ok_but_hold`), holds the unit's
//!   global cooldown: anything cast now would still be on it when the moment
//!   comes. The hold is traced as a chain step with no choice.
//!
//! What a hand-off asks of the subject, once for every class ([`suits`]):
//!
//! - it is not immune — to damage (a Divine Shield) or to new auras (a
//!   Cyclone, which also refuses the hand-off itself while it spins);
//! - its diminishing returns: it is not immune to the hand-off's category. A
//!   hand-off that shares a category with the lock it chains onto still
//!   goes: it REPLACES that lock (the engine removes a same-category crowd
//!   control when a new one lands), but only in its last `HANDOFF_LEAD`, so
//!   what it gives up is at most that, against the hand-off's own duration
//!   after diminishing returns (a Polymorph re-cast on its own Polymorph
//!   adds 5s at 50%);
//! - a hand-off that deals damage (Death Coil) does not land on a lock that
//!   breaks on damage (a Polymorph, a Freezing Trap, a Fear) — it would break
//!   the lock it chains onto;
//! - a hand-off that breaks on damage (a Polymorph, a Fear), or that makes
//!   its subject immune to it (a Cyclone), is not handed onto a subject the
//!   team is attacking or has a damage over time on.
//!
//! What a unit's hand-off does NOT need: the subject to be its own target.
//! The lock is the team's; the hand-off is whoever's tool is ready.
//!
//! Which tools count as hand-offs ([`chain_tools`]): the TARGETED casts that
//! LOCK a healer (`situation::locks_healer`: a stun, a fear or horror, a
//! polymorph, a cyclone), whose landing the model above can time. A root, a
//! snare or a silence leaves the healer healing or is not a lock. A trap
//! (Freezing Trap) lands when its victim walks onto it, an area fear
//! (Psychic Scream) on whoever stands near its caster, and a pet's stun
//! (Boar Charge) is the pet's own decision — none has a landing to time.

use bevy::prelude::*;
use smallvec::SmallVec;

use super::cast_guard::{lands_after, pre_cast_ok_but_hold, PreCastOpts};
use super::{grants_damage_immunity, holds, CombatContext, Incoming};
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::{AbilityConfig, AbilityDefinitions};
use crate::states::play_match::components::{ActiveAuras, Combatant, DRCategory};
use crate::states::play_match::constants::GCD;
use crate::states::play_match::decision_trace::{DecisionEventBuilder, RejectionReason};

/// How early before the lock's `until` a hand-off may land: the last quarter
/// second. Inside the hold rule's chain window (`CC_CHAIN_WINDOW`, 0.5s), and
/// fifteen frames — enough that an aura applied a frame late still lands
/// before the lock ends.
pub const HANDOFF_LEAD: f32 = 0.25;

/// How far ahead of a hand-off's moment its unit holds its global cooldown:
/// one global cooldown. A cast begun earlier than this is off the global
/// cooldown by the moment; one begun inside it is not.
pub const HANDOFF_HOLD: f32 = GCD;

/// The hand-off a unit owes this tick: the locked enemy, and when its lock
/// ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChainCall {
    pub subject: Entity,
    pub until: f32,
}

/// The hand-off the deciding unit owes: `Some` while its play is the Chain
/// play ([`CombatContext::chain_window`]), the lock's end is known, its
/// subject is alive and in view, and no teammate is already mid-cast on a
/// hand-off — a 1.5s Polymorph begun for this lock is the hand-off, and a
/// second one would spend a cooldown on the same moment. (A cast's target is
/// not in the snapshot, so any teammate casting a chain tool counts.)
pub fn chain_call(ctx: &CombatContext) -> Option<ChainCall> {
    let window = ctx.chain_window()?;
    let until = window.until?;
    let me = ctx.self_info()?;
    if !ctx
        .combatants
        .get(&window.subject)
        .is_some_and(|info| info.is_alive)
    {
        return None;
    }
    let in_flight = ctx.combatants.values().any(|c| {
        c.team == me.team
            && c.entity != me.entity
            && c.is_alive
            && c.casting_ability.is_some_and(is_chain_tool)
    });
    (!in_flight).then_some(ChainCall {
        subject: window.subject,
        until,
    })
}

/// The crowd control `class` hands off with, in the order its chain step
/// tries them. Exhaustive: a new class answers here. See the module docs for
/// what counts. Read by the play's probe to tell a hand-off from any other
/// cast.
pub fn chain_tools(class: CharacterClass) -> &'static [AbilityType] {
    match class {
        CharacterClass::Paladin => &[AbilityType::HammerOfJustice],
        CharacterClass::Rogue => &[AbilityType::KidneyShot],
        // Fear first: no cooldown, and the longer lock. Death Coil's horror
        // never breaks, but its damage would break a breakable lock.
        CharacterClass::Warlock => &[AbilityType::Fear, AbilityType::DeathCoil],
        CharacterClass::Mage => &[AbilityType::Polymorph],
        CharacterClass::Druid => &[AbilityType::Cyclone],
        // Freezing Trap has no landing to time, Psychic Scream no aim, and
        // the Warrior's and Shaman's kits lock nothing.
        CharacterClass::Hunter
        | CharacterClass::Priest
        | CharacterClass::Warrior
        | CharacterClass::Shaman => &[],
    }
}

/// Whether `ability` is some class's hand-off.
pub fn is_chain_tool(ability: AbilityType) -> bool {
    CharacterClass::all()
        .iter()
        .any(|class| chain_tools(*class).contains(&ability))
}

/// What the chain step does this tick, as [`plan`] rules it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChainPlan {
    /// The hand-offs DUE now, in the class's order: each lands in the lock's
    /// last [`HANDOFF_LEAD`]. The class casts the first its own gates let go.
    pub due: SmallVec<[AbilityType; 2]>,
    /// A hand-off is not due yet, but its moment comes within
    /// [`HANDOFF_HOLD`] and nothing but the lock refuses it: the unit holds
    /// its global cooldown (when nothing due fires).
    pub hold: bool,
}

/// Rule the chain step for `call`: which of the deciding unit's
/// [`chain_tools`] are due, and whether it holds its global cooldown for one
/// that is not yet. Every tool it does not mark due is rejected on `builder`
/// with its reason.
#[allow(clippy::too_many_arguments)]
pub fn plan(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    caster: &Combatant,
    caster_pos: Vec3,
    auras: Option<&ActiveAuras>,
    call: &ChainCall,
    builder: &mut DecisionEventBuilder<'_>,
) -> ChainPlan {
    let mut out = ChainPlan::default();
    let (Some(now), Some(subject_pos)) = (
        ctx.situation_now(),
        ctx.combatants.get(&call.subject).map(|i| i.position),
    ) else {
        return out;
    };
    for &tool in chain_tools(caster.class) {
        let def = abilities.get_unchecked(&tool);
        if let Err(reason) = suits(ctx, def, call.subject) {
            builder.reject(tool, reason);
            continue;
        }
        let lands_at = now + lands_after(def, caster_pos, subject_pos, auras);
        match handoff_timing(lands_at, call.until) {
            HandoffTiming::Due => {
                out.due.push(tool);
                continue;
            }
            HandoffTiming::Late => {}
            HandoffTiming::Early { wait } => {
                if !out.hold
                    && wait <= HANDOFF_HOLD
                    && pre_cast_ok_but_hold(
                        tool,
                        def,
                        caster,
                        caster_pos,
                        auras,
                        Some((call.subject, subject_pos)),
                        ctx,
                        PreCastOpts::default(),
                    )
                {
                    out.hold = true;
                }
            }
        }
        builder.reject(
            tool,
            RejectionReason::MissesHandoff {
                lands_at,
                until: call.until,
            },
        );
    }
    out
}

/// Where a hand-off landing at `lands_at` falls against a lock ending at
/// `until`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandoffTiming {
    /// In the lock's last [`HANDOFF_LEAD`], `until` included.
    Due,
    /// Before it: due in `wait` seconds.
    Early { wait: f32 },
    /// After the lock has ended.
    Late,
}

/// [`HandoffTiming`] for a hand-off landing at `lands_at` onto a lock ending
/// at `until`. Pure.
pub fn handoff_timing(lands_at: f32, until: f32) -> HandoffTiming {
    if lands_at > until {
        HandoffTiming::Late
    } else if lands_at >= until - HANDOFF_LEAD {
        HandoffTiming::Due
    } else {
        HandoffTiming::Early {
            wait: until - HANDOFF_LEAD - lands_at,
        }
    }
}

/// Whether the hand-off `def` suits `subject` at all, whenever it lands (the
/// module docs' list). `Err` with the reason to trace.
pub fn suits(
    ctx: &CombatContext,
    def: &AbilityConfig,
    subject: Entity,
) -> Result<(), RejectionReason> {
    if ctx.entity_is_immune(subject) {
        return Err(RejectionReason::TargetImmune);
    }
    let Some(aura) = def.applies_aura.as_ref() else {
        return Err(RejectionReason::NoValidTarget);
    };
    let locks: SmallVec<[_; 2]> = ctx
        .active_auras
        .get(&subject)
        .into_iter()
        .flatten()
        .filter(|a| holds(a.effect_type))
        .collect();
    if let Some(category) = aura
        .dr_category
        .or_else(|| DRCategory::from_aura_type(&aura.aura_type))
    {
        if ctx.is_dr_immune(subject, category) {
            return Err(RejectionReason::DRImmune { category });
        }
    }
    if def.damage_base_max > 0.0
        && locks
            .iter()
            .any(|lock| lock.break_on_damage_threshold >= 0.0)
    {
        return Err(RejectionReason::PreconditionUnmet {
            note: "hand-off's damage would break the lock it chains onto".into(),
        });
    }
    let spoils_damage =
        Incoming::of(def) == Incoming::Breaks || grants_damage_immunity(aura.aura_type);
    if spoils_damage
        && (ctx.attacked_by_my_team(subject) || ctx.has_friendly_dots_on_target(subject))
    {
        return Err(RejectionReason::PreconditionUnmet {
            note: "hand-off would be broken by, or shield the subject from, the team's damage"
                .into(),
        });
    }
    Ok(())
}
