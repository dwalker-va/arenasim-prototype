//! The Priest's Shadow talent abilities (AS-416): Mind Flay, Vampiric Embrace
//! and Silence. (Blackout is a proc, `talents.ron`; Shadowform is AS-413's.)
//!
//! Each is a `talent: true` ability, in a Priest's book only when its build
//! takes the node that grants it, so each rule first asks the book and does
//! nothing at all without it: an untalented Priest decides exactly as it did
//! before these existed (`tests/talent_ai_audit.rs` holds every rule to both
//! halves).
//!
//! - **Mind Flay** is a 3s channel that slows its target. A healing Priest
//!   channels it as a PEEL ([`mind_flay_peel_target`]): on an enemy melee or
//!   pet on one of its ranged teammates (itself included), only while no heal
//!   or dispel is owed and no walk owns its legs. A Shadowform Priest
//!   channels it as its FILLER on its kill target, below Mind Blast; it
//!   closes to Mind Flay's reach to do so (`combat_core::movement`).
//! - **Vampiric Embrace** goes on the Priest's target ahead of its Shadow
//!   damage, while the target lacks the Priest's own: in the Shadowform
//!   rotation after Shadow Word: Pain, and in a healing Priest's damage slot
//!   ahead of Mind Blast, once nothing is owed.
//! - **Silence**, castable only in Shadowform (a choice rule locks it
//!   otherwise), on an enemy mid-cast — a healer first — whose cast a silence
//!   stops ([`silence_target`]).
//!
//! The decisions are pure ([`mind_flay_peel_target`], [`silence_target`]);
//! the casts are `try_*` steps the Priest's chains run
//! (`priest::decide_priest_action`, `priest::decide_shadow_priest_action`).
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_book::AbilityBook;
use crate::states::play_match::ability_config::AbilityConfig;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::GCD;
use crate::states::play_match::decision_trace::{DecisionEventBuilder, RejectionReason};
use crate::states::play_match::utils::log_ability_use;

use super::cast_guard::{cast_gate, unreached_reason, PreCastOpts};
use super::{cast_reach, CastReach, CombatContext, CombatantInfo};

/// How close an enemy melee or pet must stand to the teammate it targets for
/// a healing Priest's Mind Flay to peel it: on the teammate, or a step from
/// it.
pub const MIND_FLAY_PEEL_REACH: f32 = 8.0;

/// Yards inside Mind Flay's range a Shadowform Priest stops at, so drift
/// between decisions does not flicker its filler in and out of range.
pub const MIND_FLAY_RANGE_BUFFER: f32 = 2.0;

/// Whether `auras` hold an impairment that makes Mind Flay's slow pointless:
/// a slow or a root already on it.
fn impaired(auras: Option<&Vec<Aura>>) -> bool {
    auras.is_some_and(|list| {
        list.iter()
            .any(|a| matches!(a.effect_type, AuraType::MovementSpeedSlow | AuraType::Root))
    })
}

/// The enemy a healing Priest's Mind Flay peels, among the visible living
/// enemies: a melee class or a pet, targeting a living ranged teammate of
/// `me` (the Priest included; a melee teammate wants no escape) and within
/// [`MIND_FLAY_PEEL_REACH`] of it, not already slowed or rooted, not held in
/// crowd control and not immune to damage — and `reaches` (Mind Flay's range
/// and sight). The nearest to the Priest; entity order breaks a tie.
pub fn mind_flay_peel_target(
    ctx: &CombatContext,
    me: Entity,
    my_pos: Vec3,
    reaches: impl Fn(&CombatantInfo) -> bool,
) -> Option<Entity> {
    let team = ctx.combatants.get(&me)?.team;
    ctx.combatants
        .values()
        .filter(|e| e.team != team && e.is_alive && (e.is_pet || e.class.is_melee()))
        .filter(|e| {
            e.target
                .and_then(|t| ctx.combatants.get(&t))
                .is_some_and(|v| {
                    v.team == team
                        && v.is_alive
                        && !v.is_pet
                        && !v.class.is_melee()
                        && e.position.distance(v.position) <= MIND_FLAY_PEEL_REACH
                })
        })
        .filter(|e| {
            !impaired(ctx.active_auras.get(&e.entity))
                && ctx
                    .held_in_hard_cc(e.entity, super::Incoming::Breaks)
                    .is_none()
                && !ctx.entity_is_immune(e.entity)
        })
        .filter(|e| reaches(e))
        .min_by(|a, b| {
            my_pos
                .distance(a.position)
                .total_cmp(&my_pos.distance(b.position))
        })
        .map(|e| e.entity)
}

/// The enemy Silence goes on, among the visible living enemies (pets aside):
/// one mid-cast, whose cast a silence stops (`AbilityConfig::silence_stops`,
/// by a mana user), that is not immune to it (Aura Mastery), already
/// silenced, immune to its diminishing returns or immune to damage, and that
/// `reaches` accepts (Silence's range and sight). A healer first, then the
/// rest; entity order within each.
pub fn silence_target(
    ctx: &CombatContext,
    me: Entity,
    reaches: impl Fn(&CombatantInfo) -> bool,
) -> Option<Entity> {
    let mut candidates = silence_candidates(ctx, me);
    candidates.retain(|e| reaches(e));
    candidates
        .iter()
        .find(|e| e.is_healer())
        .or_else(|| candidates.first())
        .map(|e| e.entity)
}

/// Every enemy [`silence_target`] would weigh, reach aside.
fn silence_candidates<'c>(ctx: &'c CombatContext, me: Entity) -> Vec<&'c CombatantInfo> {
    let Some(team) = ctx.combatants.get(&me).map(|c| c.team) else {
        return Vec::new();
    };
    ctx.combatants
        .values()
        .filter(|e| e.team != team && e.is_alive && !e.is_pet && e.class.uses_mana())
        .filter(|e| {
            e.casting_ability
                .and_then(|a| e.book.get(&a))
                .is_some_and(AbilityConfig::silence_stops)
        })
        .filter(|e| {
            !ctx.entity_is_interrupt_immune(e.entity)
                && !ctx.entity_is_immune(e.entity)
                && !ctx.is_dr_immune(e.entity, DRCategory::Silence)
                && !ctx
                    .active_auras
                    .get(&e.entity)
                    .is_some_and(|list| list.iter().any(|a| a.effect_type == AuraType::Silence))
        })
        .collect()
}

/// Begin Mind Flay's channel on `target`: its mana paid up front (as Drain
/// Life's, an interrupted channel is not refunded), the global cooldown, and
/// its slow on the target, which ends with the channel.
fn channel_mind_flay(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    def: &AbilityConfig,
    entity: Entity,
    combatant: &mut Combatant,
    ctx: &CombatContext,
    target: Entity,
) {
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    let tick_interval = def.channel_tick_interval;
    commands.entity(entity).insert(ChannelingState {
        ability: AbilityType::MindFlay,
        duration_remaining: def.channel_duration.unwrap_or(3.0),
        time_until_next_tick: tick_interval,
        tick_interval,
        target,
        interrupted: false,
        interrupted_display_time: 0.0,
        ticks_applied: 0,
    });
    if let Some(pending) = AuraPending::from_ability(target, entity, def) {
        commands.spawn(pending);
    }
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        ctx.combatants.get(&target).map(|info| info.log_id()),
        "begins channeling",
    );
}

/// A healing Priest's Mind Flay: the peel ([`mind_flay_peel_target`]). Held
/// while a walk owns the Priest's legs (`escape_defer`: the channel would
/// plant it) and while a heal or dispel is owed (`held`, the reason).
pub fn try_mind_flay_peel(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    escape_defer: Option<f32>,
    held: Option<&'static str>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::MindFlay;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    let note = if escape_defer.is_some() {
        Some("dip/escape/dispel/medic walk live: movement-locking channel deferred")
    } else {
        held
    };
    if let Some(note) = note {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: note.to_string(),
            },
        );
        return false;
    }
    let reach = |e: &CombatantInfo| cast_reach(ctx, def.range, my_pos, e.position);
    let Some(target) =
        mind_flay_peel_target(ctx, entity, my_pos, |e| reach(e) == CastReach::Reaches)
    else {
        // A chaser it would peel but cannot reach is reported as such.
        let unreached = mind_flay_peel_target(ctx, entity, my_pos, |_| true)
            .and_then(|e| ctx.combatants.get(&e))
            .map(|e| match reach(e) {
                CastReach::LosBlocked => RejectionReason::LosBlocked,
                CastReach::OutOfRange { distance } => RejectionReason::OutOfRange {
                    distance,
                    max: def.range,
                },
                CastReach::Reaches => RejectionReason::NoValidTarget,
            });
        let reason = match unreached {
            Some(reason) => unreached_reason(
                reason,
                ability,
                def,
                combatant,
                my_pos,
                auras,
                ctx,
                PreCastOpts::default(),
            ),
            None => RejectionReason::PreconditionUnmet {
                note: "no enemy melee or pet on a ranged teammate to slow".into(),
            },
        };
        builder.reject(ability, reason);
        return false;
    };
    let Some(info) = ctx.combatants.get(&target) else {
        return false;
    };
    let opts = PreCastOpts {
        check_friendly_cc: true,
        check_target_immune: true,
        ..Default::default()
    };
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target, info.position)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };
    builder.choose(cleared, Some(target), false);
    channel_mind_flay(commands, combat_log, def, entity, combatant, ctx, target);
    true
}

/// A Shadowform Priest's Mind Flay: the filler, on its target.
pub fn try_mind_flay_filler(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::MindFlay;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    let Some((target, target_pos)) = combatant
        .target
        .and_then(|t| ctx.combatants.get(&t))
        .filter(|info| info.is_alive)
        .map(|info| (info.entity, info.position))
    else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let opts = PreCastOpts {
        check_friendly_cc: true,
        check_target_immune: true,
        ..Default::default()
    };
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target, target_pos)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };
    builder.choose(cleared, Some(target), false);
    channel_mind_flay(commands, combat_log, def, entity, combatant, ctx, target);
    true
}

/// The stop distance of a Shadowform Priest's pursuit, when its book holds a
/// Mind Flay its choices leave castable: inside Mind Flay's range by
/// [`MIND_FLAY_RANGE_BUFFER`], so its filler reaches. `None` otherwise: it
/// keeps its class's.
pub fn shadow_pursuit_range(combatant: &Combatant) -> Option<f32> {
    if combatant.priest_form != crate::states::match_config::PriestForm::Shadowform {
        return None;
    }
    let book = combatant.book();
    if book.is_locked(AbilityType::MindFlay) {
        return None;
    }
    book.get(&AbilityType::MindFlay)
        .map(|def| def.range - MIND_FLAY_RANGE_BUFFER)
}

/// Vampiric Embrace on the Priest's target, ahead of its Shadow damage: while
/// the target lacks the Priest's own. A healing Priest holds it while a heal
/// or dispel is owed (`held`, the reason); a Shadowform Priest passes `None`.
pub fn try_vampiric_embrace(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    held: Option<&'static str>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::VampiricEmbrace;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    if let Some(note) = held {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: note.to_string(),
            },
        );
        return false;
    }
    let Some((target, target_pos)) = combatant
        .target
        .and_then(|t| ctx.combatants.get(&t))
        .filter(|info| info.is_alive)
        .map(|info| (info.entity, info.position))
    else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let own = ctx.active_auras.get(&target).is_some_and(|list| {
        list.iter()
            .any(|a| a.effect_type == AuraType::LeechMark && a.caster == Some(entity))
    });
    if own {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }
    let opts = PreCastOpts {
        check_friendly_cc: true,
        check_target_immune: true,
        ..Default::default()
    };
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target, target_pos)),
        ctx,
        opts,
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };
    builder.choose(cleared, Some(target), true);
    combatant.current_mana -= def.mana_cost;
    combatant.start_cooldown(ability);
    combatant.global_cooldown = GCD;
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        ctx.combatants.get(&target).map(|info| info.log_id()),
        "casts",
    );
    if let Some(pending) = AuraPending::from_ability(target, entity, def) {
        commands.spawn(pending);
    }
    true
}

/// Silence on an enemy mid-cast, a healer first ([`silence_target`]).
/// Queued for the same frame's later deciders as the Priest's other instant
/// crowd control is, so a second silencer does not double it.
pub fn try_silence(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::Silence;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    let reach = |e: &CombatantInfo| cast_reach(ctx, def.range, my_pos, e.position);
    let Some(target) = silence_target(ctx, entity, |e| reach(e) == CastReach::Reaches) else {
        // A caster it would silence but cannot reach is reported as such.
        let unreached = silence_target(ctx, entity, |_| true)
            .and_then(|e| ctx.combatants.get(&e))
            .map(|e| match reach(e) {
                CastReach::LosBlocked => RejectionReason::LosBlocked,
                CastReach::OutOfRange { distance } => RejectionReason::OutOfRange {
                    distance,
                    max: def.range,
                },
                CastReach::Reaches => RejectionReason::NoValidTarget,
            });
        let reason = match unreached {
            Some(reason) => unreached_reason(
                reason,
                ability,
                def,
                combatant,
                my_pos,
                auras,
                ctx,
                PreCastOpts::default(),
            ),
            None => RejectionReason::PreconditionUnmet {
                note: "no enemy healer or caster mid-cast a silence would stop".into(),
            },
        };
        builder.reject(ability, reason);
        return false;
    };
    let Some(info) = ctx.combatants.get(&target) else {
        return false;
    };
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target, info.position)),
        ctx,
        PreCastOpts::default(),
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };
    builder.choose(cleared, Some(target), true);
    combatant.current_mana -= def.mana_cost;
    combatant.start_cooldown(ability);
    combatant.global_cooldown = GCD;
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        Some(info.log_id()),
        "casts",
    );
    if let Some(pending) = AuraPending::from_ability(target, entity, def) {
        same_frame_cc_queue.push((target, pending.aura.clone()));
        commands.spawn(pending);
    }
    true
}
