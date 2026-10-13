//! The interrupt decision: whom Kick, Pummel and Wind Shear interrupt, and
//! whom the Felhunter's Spell Lock does.
//!
//! Interrupts are decided outside the class dispatch — Kick, Pummel and Wind
//! Shear in `combat_ai::check_interrupts`, which runs after every new cast of
//! the frame has started, and Spell Lock in the Felhunter's own AI — but they
//! choose their target by the rule every targeted cast does: range, then line
//! of sight ([`cast_reach`]), the two gates `pre_cast_ok` applies at cast
//! start. A caster scan ranks only the casters it REACHES, so one behind a
//! pillar yields to one in sight instead of being picked and then refused;
//! a cast refused for sight alone is traced `LosBlocked`.
//!
//! [`decide_interrupt`] is pure over a [`CombatContext`] and the frame's
//! [`EnemyCast`]s, so a scene (`Scene::interrupt`) runs exactly what the
//! frame runs.

use std::collections::BTreeMap;

use bevy::prelude::*;

use super::cast_guard::{cast_gate, Cleared, PreCastOpts};
use super::{cast_reach, priority_interrupt_target, CastReach, CombatContext, InterruptCandidate};
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::{AbilityConfig, AbilityDefinitions};
use crate::states::play_match::components::{
    ActiveAuras, CastingState, ChannelingState, Combatant,
};
use crate::states::play_match::decision_trace::{DecisionTrace, RejectionReason};

/// The interrupt a class casts itself, or `None` for a class with none.
/// Exhaustive, so a new class has to say whether it interrupts; the Warlock's
/// interrupt is its Felhunter's (Spell Lock, decided by the pet's AI).
/// `tests/interrupt_sight.rs` holds this to every non-pet `is_interrupt`
/// ability in `abilities.ron`.
pub fn class_interrupt(class: CharacterClass) -> Option<AbilityType> {
    match class {
        CharacterClass::Warrior => Some(AbilityType::Pummel),
        CharacterClass::Rogue => Some(AbilityType::Kick),
        CharacterClass::Shaman => Some(AbilityType::WindShear),
        CharacterClass::Mage
        | CharacterClass::Priest
        | CharacterClass::Warlock
        | CharacterClass::Paladin
        | CharacterClass::Hunter
        | CharacterClass::Druid => None,
    }
}

/// What an interrupt would do to a cast or channel in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastTake {
    /// It cuts it ([`super::cast_is_interruptible`]; a channel not already
    /// interrupted).
    Takes,
    /// The cast cannot be interrupted (Aimed Shot,
    /// [`super::uninterruptible_cast`]): passed over, and traced.
    Uninterruptible,
    /// Nothing to cut: already interrupted, or an instant routed through a
    /// zero-length cast.
    Nothing,
}

impl CastTake {
    /// A cast of `def`, interrupted already or not.
    pub fn of_cast(def: &AbilityConfig, interrupted: bool) -> Self {
        if interrupted {
            CastTake::Nothing
        } else if def.can_be_interrupted() {
            CastTake::Takes
        } else if def.has_cast_bar() && def.uninterruptible {
            CastTake::Uninterruptible
        } else {
            CastTake::Nothing
        }
    }
}

/// A unit mid a cast or a channel, as an interrupter scans it.
#[derive(Clone, Copy, Debug)]
pub struct EnemyCast {
    pub entity: Entity,
    pub team: u8,
    pub class: CharacterClass,
    pub alive: bool,
    pub position: Vec3,
    /// The ability being cast or channelled.
    pub ability: AbilityType,
    /// A channel rather than a cast.
    pub channel: bool,
    pub take: CastTake,
}

impl EnemyCast {
    /// A unit with a `CastingState`.
    pub fn casting(
        entity: Entity,
        combatant: &Combatant,
        position: Vec3,
        cast: &CastingState,
        abilities: &AbilityDefinitions,
    ) -> Self {
        let take = CastTake::of_cast(abilities.get_unchecked(&cast.ability), cast.interrupted);
        debug_assert_eq!(
            take == CastTake::Takes,
            super::cast_is_interruptible(cast, abilities)
        );
        debug_assert_eq!(
            take == CastTake::Uninterruptible,
            super::uninterruptible_cast(cast, abilities).is_some()
        );
        Self {
            entity,
            team: combatant.team,
            class: combatant.class,
            alive: combatant.is_alive(),
            position,
            ability: cast.ability,
            channel: false,
            take,
        }
    }

    /// A unit with a `ChannelingState`.
    pub fn channeling(
        entity: Entity,
        combatant: &Combatant,
        position: Vec3,
        channel: &ChannelingState,
    ) -> Self {
        Self {
            entity,
            team: combatant.team,
            class: combatant.class,
            alive: combatant.is_alive(),
            position,
            ability: channel.ability,
            channel: true,
            take: if channel.interrupted {
                CastTake::Nothing
            } else {
                CastTake::Takes
            },
        }
    }
}

/// What one interrupter decided this frame.
#[derive(Debug)]
pub enum InterruptDecision {
    /// Interrupt `target`'s `cast`.
    Interrupt {
        cleared: Cleared,
        target: Entity,
        cast: AbilityType,
    },
    /// Passed over `target` for `reason`, which is traced: an uninterruptible
    /// cast, or one it reaches in range but cannot see.
    Refuse {
        ability: AbilityType,
        target: Entity,
        reason: RejectionReason,
    },
    /// Nothing to interrupt, or the interrupter cannot cast its interrupt (its
    /// own state, its resource, its range): nothing traced.
    Idle,
}

/// The interrupt decision of the unit `ctx` is built for: whom its class's
/// interrupt ([`class_interrupt`]) takes, from `casts` — every unit mid a cast
/// or a channel this frame, in entity order.
///
/// First the cast every interrupter takes ahead of its usual pick, an enemy
/// Druid's Cyclone ([`priority_interrupt_target`]); then the Shaman's pick from
/// every enemy cast it reaches (the healer, then the nearest, then the lowest
/// entity), and the Warrior's and Rogue's kill target. Every candidate a scan
/// ranks is one the interrupt REACHES ([`cast_reach`]: range, then sight).
pub fn decide_interrupt(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    combatant: &Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    casts: &BTreeMap<Entity, EnemyCast>,
) -> InterruptDecision {
    use InterruptDecision::Idle;
    if !combatant.is_alive() {
        return Idle;
    }
    // Nothing is cast while stunned, feared or polymorphed.
    if crate::states::play_match::utils::is_incapacitated(auras) {
        return Idle;
    }
    let Some(ability) = class_interrupt(combatant.class) else {
        return Idle;
    };
    let def = abilities.get_unchecked(&ability);
    let reach = |at: Vec3| cast_reach(ctx, def.range, my_pos, at);

    // The enemy casts (not channels) a scan considers: alive, not immune.
    let scanned = || {
        casts.values().filter(|c| {
            !c.channel && c.team != combatant.team && c.alive && !ctx.entity_is_immune(c.entity)
        })
    };

    let priority = priority_interrupt_target(
        scanned()
            .filter(|c| c.take == CastTake::Takes && reach(c.position) == CastReach::Reaches)
            .map(|c| InterruptCandidate {
                entity: c.entity,
                ability: c.ability,
                distance: my_pos.distance(c.position),
            }),
    );

    let target = if let Some(priority) = priority {
        priority
    } else if combatant.class == CharacterClass::Shaman {
        // The healer first, then the nearest, then the lowest entity id.
        let mut best: Option<(Entity, bool, f32)> = None;
        // The nearest reached enemy mid an UNINTERRUPTIBLE cast: passed over,
        // traced when nothing else is casting.
        let mut passed_over: Option<(Entity, f32)> = None;
        // The nearest enemy in range mid a cast Wind Shear could take, but
        // behind cover: traced when there is nothing else.
        let mut unseen: Option<(Entity, f32)> = None;
        let nearer = |slot: Option<(Entity, f32)>, e: Entity, d: f32| {
            slot.is_none_or(|(pe, pd)| d < pd || (d == pd && e < pe))
        };
        for c in scanned() {
            let dist = my_pos.distance(c.position);
            match (reach(c.position), c.take) {
                (CastReach::Reaches, CastTake::Takes) => {}
                (CastReach::Reaches, CastTake::Uninterruptible) => {
                    if nearer(passed_over, c.entity, dist) {
                        passed_over = Some((c.entity, dist));
                    }
                    continue;
                }
                (CastReach::LosBlocked, CastTake::Takes) => {
                    if nearer(unseen, c.entity, dist) {
                        unseen = Some((c.entity, dist));
                    }
                    continue;
                }
                _ => continue,
            }
            let is_healer = c.class.is_healer();
            let replace = match best {
                None => true,
                Some((be, bh, bd)) => {
                    if is_healer != bh {
                        is_healer
                    } else if dist != bd {
                        dist < bd
                    } else {
                        c.entity < be
                    }
                }
            };
            if replace {
                best = Some((c.entity, is_healer, dist));
            }
        }
        match (best, passed_over, unseen) {
            (Some((e, _, _)), _, _) | (None, Some((e, _)), _) | (None, None, Some((e, _))) => e,
            (None, None, None) => return Idle,
        }
    } else {
        // The Warrior and the Rogue interrupt their kill target.
        let Some(target) = combatant.target else {
            return Idle;
        };
        target
    };

    // Not on an immune target (Divine Shield, Cyclone).
    if ctx.entity_is_immune(target) {
        return Idle;
    }
    let Some(cast) = casts.get(&target) else {
        return Idle; // Not casting or channelling.
    };
    let uninterruptible = match cast.take {
        CastTake::Takes => false,
        CastTake::Uninterruptible => true,
        CastTake::Nothing => return Idle,
    };
    // A Rogue breaks stealth before it kicks.
    if ability == AbilityType::Kick && combatant.stealthed {
        return Idle;
    }
    // The gates every targeted cast passes: its own state (Travel Form, a
    // school lockout, a silence, the cooldown), its resource, its range, and
    // last its sight.
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target, cast.position)),
        ctx,
        PreCastOpts::default(),
    ) {
        Ok(cleared) => cleared,
        Err(RejectionReason::LosBlocked) => {
            return InterruptDecision::Refuse {
                ability,
                target,
                reason: RejectionReason::LosBlocked,
            }
        }
        Err(_) => return Idle,
    };
    if uninterruptible {
        return InterruptDecision::Refuse {
            ability,
            target,
            reason: RejectionReason::Uninterruptible {
                ability: cast.ability,
            },
        };
    }
    InterruptDecision::Interrupt {
        cleared,
        target,
        cast: cast.ability,
    }
}

/// [`decide_interrupt`], traced: an interrupt is recorded as chosen, a refusal
/// as its rejection, and nothing at all when the unit was idle. Returns the
/// interrupt to cast, its target and the cast it cuts.
#[allow(clippy::too_many_arguments)]
pub fn interrupt_turn(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    combatant: &Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    casts: &BTreeMap<Entity, EnemyCast>,
    trace: Option<&mut DecisionTrace>,
) -> Option<(AbilityType, Entity, AbilityType)> {
    let decision = decide_interrupt(ctx, abilities, combatant, my_pos, auras, casts);
    let target = match &decision {
        InterruptDecision::Interrupt { target, .. } | InterruptDecision::Refuse { target, .. } => {
            *target
        }
        InterruptDecision::Idle => return None,
    };
    // Through the context, so the event carries the team's situation as every
    // other ability decision does.
    let mut builder =
        trace.and_then(|trace| ctx.start_ability_decision(trace, Some(target), my_pos));
    let outcome = match decision {
        InterruptDecision::Interrupt {
            cleared,
            target,
            cast,
        } => {
            let ability = cleared.ability();
            if let Some(b) = builder.as_mut() {
                b.choose(cleared, Some(target), true);
            }
            Some((ability, target, cast))
        }
        InterruptDecision::Refuse {
            ability, reason, ..
        } => {
            if let Some(b) = builder.as_mut() {
                b.reject(ability, reason);
            }
            None
        }
        InterruptDecision::Idle => None,
    };
    if let Some(b) = builder {
        b.finish();
    }
    outcome
}

/// Whom Spell Lock interrupts, from the casts then the channels it scans, each
/// in the order its query yields them: an enemy Druid's Cyclone first, then
/// the first heal, then the first cast of any kind
/// ([`super::pet_ai::spell_lock_target`]), and with no cast, the first
/// channel. Every candidate is one Spell Lock REACHES ([`cast_reach`]). With
/// none, the reason: an uninterruptible cast it reached, an interruptible one
/// it could reach but not see, or no target at all.
pub fn spell_lock_pick(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    my_team: u8,
    my_pos: Vec3,
    casts: &[EnemyCast],
) -> Result<Entity, RejectionReason> {
    let range = abilities.get_unchecked(&AbilityType::SpellLock).range;
    let mut reachable: Vec<(InterruptCandidate, bool)> = Vec::new();
    let mut passed_over: Option<AbilityType> = None;
    let mut unseen = false;
    let considered = casts
        .iter()
        .filter(|c| c.team != my_team && c.alive && !ctx.entity_is_immune(c.entity));
    for c in considered.clone().filter(|c| !c.channel) {
        match cast_reach(ctx, range, my_pos, c.position) {
            CastReach::OutOfRange { .. } => continue,
            CastReach::LosBlocked => {
                unseen |= c.take == CastTake::Takes;
                continue;
            }
            CastReach::Reaches => {}
        }
        match c.take {
            CastTake::Takes => reachable.push((
                InterruptCandidate {
                    entity: c.entity,
                    ability: c.ability,
                    distance: my_pos.distance(c.position),
                },
                abilities.get_unchecked(&c.ability).is_heal(),
            )),
            CastTake::Uninterruptible => passed_over = passed_over.or(Some(c.ability)),
            CastTake::Nothing => {}
        }
    }
    if let Some(target) = super::pet_ai::spell_lock_target(&reachable) {
        return Ok(target);
    }
    for c in considered.filter(|c| c.channel) {
        match cast_reach(ctx, range, my_pos, c.position) {
            CastReach::Reaches => return Ok(c.entity),
            CastReach::LosBlocked => unseen = true,
            CastReach::OutOfRange { .. } => {}
        }
    }
    Err(match (passed_over, unseen) {
        (Some(cast), _) => RejectionReason::Uninterruptible { ability: cast },
        (None, true) => RejectionReason::LosBlocked,
        (None, false) => RejectionReason::NoValidTarget,
    })
}
