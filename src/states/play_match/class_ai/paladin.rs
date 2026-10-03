//! Paladin AI Module
//!
//! Holy warrior and healer - combines healing with melee utility.
//!
//! ## Priority Order
//! 1. Paladin Aura (buff all allies pre-combat — Devotion/Shadow Resistance/Concentration)
//!    1.5. Divine Shield (emergency: self < 30% HP, or CC break for teammate)
//! 2. Cleanse - Urgent (Polymorph, Freezing Trap, UA Silence, Fear on allies)
//! 3. Emergency healing (ally < 40% HP) - Holy Shock (heal)
//! 4. Hammer of Justice (stun enemy in melee range)
//! 5. Standard healing (ally < 90% HP) - Flash of Light
//! 6. Holy Light (ally 50-85% HP, safe to cast long heal)
//! 7. Cleanse - Maintenance (roots, DoTs when team stable)
//! 8. Holy Shock (damage) - when team healthy
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;
use std::collections::BTreeMap;

use crate::combat::log::CombatLog;
use crate::states::match_config::{CharacterClass, PaladinAura};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::{AbilityConfig, AbilityDefinitions};
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{
    CRITICAL_HP_THRESHOLD, DIVINE_SHIELD_HP_THRESHOLD, DIVINE_SHIELD_MIN_CC_REMAINING, GCD,
    HEALTHY_HP_THRESHOLD, HOLY_SHOCK_DAMAGE_RANGE, LOW_HP_THRESHOLD, SAFE_HEAL_MAX_THRESHOLD,
};
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, MovementGoalKind, MovementTrigger,
    Posture as TracePosture, RejectionReason,
};
use crate::states::play_match::utils::{combatant_id, log_ability_use};

use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, unreached_reason, PreCastOpts};
use super::healer_postures::start_movement_event_with_target;

use super::{CombatContext, CombatantInfo};

/// Per-tick output of [`evaluate_paladin_posture`], threaded into
/// [`decide_paladin_action`] (mirrors the Priest's `escape_defer` but adds
/// the Hammer of Justice gate).
pub struct PaladinMovementPlan {
    /// `Some(urgency_hp_threshold)` while an ESCAPE window, a DIP, a dispel
    /// walk or a medic walk is live: the heal ladder defers non-critical movement-locking casts
    /// (Flash of Light, Holy Light) whose would-be target is ABOVE the
    /// threshold — casting locks movement, and an undeferred heal mid-dip would
    /// stall the walk into a budget abort (R8; same rule as the Priest's R7),
    /// or mid-dispel-walk would hold the Paladin out of Cleanse range while
    /// its trapped teammate's CC runs.
    pub cast_defer: Option<f32>,
    /// Hammer of Justice gate for this tick (reservation / dip cast).
    pub hoj: HojPlan,
}

impl Default for PaladinMovementPlan {
    fn default() -> Self {
        Self {
            cast_defer: None,
            hoj: HojPlan::Rotation,
        }
    }
}

/// How the rotation may use Hammer of Justice this tick.
pub enum HojPlan {
    /// No reservation: rotation HoJ behaves exactly as it did pre-U8
    /// (no living enemy healer, or the reservation is released because the
    /// Paladin is PRESSURED/ESCAPE — self-peel is never starved).
    Rotation,
    /// A living enemy healer exists and the Paladin is not pressured:
    /// rotation HoJ is suppressed — the cooldown is saved for dips.
    Reserved,
    /// Mid-dip and within HoJ range of the dip target: cast HoJ on this
    /// target now. On a successful cast the caller installs
    /// `completed_state` (posture back to FREE — DipComplete) and removes
    /// the walk directive.
    DipCast {
        target: Entity,
        completed_state: HealerPosture,
    },
}

/// Pure reservation predicate (R8): rotation HoJ is allowed unless a living
/// enemy healer exists AND the Paladin is not under pressure. PRESSURED and
/// ESCAPE release the reservation (self-peel on the Paladin's own attacker
/// is never starved); FREE and DIP keep it (the dip path casts through
/// [`HojPlan::DipCast`], never through the rotation).
pub fn rotation_hoj_allowed(posture: Posture, enemy_healer_alive: bool) -> bool {
    !enemy_healer_alive || matches!(posture, Posture::Pressured | Posture::Escape)
}

/// Per-target Hammer of Justice eligibility — the exact filter set the
/// rotation's target scan applies (alive enemy non-pet, not damage-immune, not
/// stun-DR-immune; unseen enemies are never in `ctx.combatants`). Shared by the rotation, the DIP entry
/// predicate, and the DIP arrival/abort re-checks so the dip can never walk
/// toward a guaranteed-rejected cast (R8).
pub fn hoj_target_eligible(ctx: &CombatContext, my_team: u8, target: Entity) -> bool {
    let Some(info) = ctx.combatants.get(&target) else {
        return false;
    };
    info.team != my_team
        && info.current_health > 0.0
        && !info.is_pet
        && !ctx.entity_is_immune(target)
        && !ctx.is_dr_immune(target, DRCategory::Stuns)
}

/// DIP target selection (pure over the snapshot): the nearest living enemy
/// healer that is HoJ-eligible and within `reach` of `my_pos`. Ties resolve
/// to the first in BTree order (deterministic). `None` when no enemy healer
/// is reachable — no dip.
pub fn dip_target_candidate(
    ctx: &CombatContext,
    my_team: u8,
    my_pos: Vec3,
    reach: f32,
) -> Option<Entity> {
    ctx.alive_enemies()
        .into_iter()
        .filter(|e| e.class.is_healer())
        .filter(|e| hoj_target_eligible(ctx, my_team, e.entity))
        .filter(|e| my_pos.distance(e.position) <= reach)
        .min_by(|a, b| {
            my_pos
                .distance(a.position)
                .partial_cmp(&my_pos.distance(b.position))
                .unwrap()
        })
        .map(|e| e.entity)
}

/// Paladin AI: Decides and executes abilities for a Paladin combatant.
///
/// `plan` is the movement-AI output for this tick (U8): cast deferral while
/// an ESCAPE window or DIP is live, plus the Hammer of Justice gate
/// (reservation for the enemy-healer dip / the dip cast itself).
pub fn decide_paladin_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    paladin_aura_this_frame: &mut std::collections::HashSet<Entity>,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    plan: &PaladinMovementPlan,
    decision_trace: &mut DecisionTrace,
) -> bool {
    // GCD short-circuit — no event.
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, combatant.target, my_pos)
    else {
        return false;
    };

    // Priority 1: Paladin Aura.
    if try_paladin_aura(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        paladin_aura_this_frame,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 1.5: Divine Shield (emergency defensive).
    if try_divine_shield(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        auras,
        ctx,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 2: Cleanse - Urgent (Polymorph, Freezing Trap, UA Silence, Fear).
    if try_cleanse(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        super::URGENT_DISPEL_PRIORITY,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 2.5: DIP Hammer of Justice (U8). The dip walked up to
    // dip_budget seconds for exactly this cast — it outranks everything
    // below the urgent dispel. On success the posture returns to FREE
    // (DipComplete) and the walk directive dies with it; the return to the
    // kill target happens naturally via FREE's legacy melee pursuit.
    if let HojPlan::DipCast {
        target,
        completed_state,
    } = &plan.hoj
    {
        if try_dip_hammer_of_justice(
            commands,
            combat_log,
            abilities,
            entity,
            combatant,
            my_pos,
            auras,
            ctx,
            *target,
            same_frame_cc_queue,
            &mut builder,
        ) {
            // `builder` exclusively borrows the trace; finish it before
            // emitting the DipComplete movement event.
            builder.finish();
            commands.entity(entity).try_insert(*completed_state);
            commands.entity(entity).remove::<MovementDirective>();
            if let Some(mut mbuilder) =
                start_movement_event_with_target(decision_trace, ctx, *target, my_pos)
            {
                mbuilder.transition(
                    TracePosture::Dip,
                    TracePosture::Free,
                    MovementTrigger::DipComplete,
                    MovementGoalKind::Entity,
                );
                mbuilder.finish();
            }
            return true;
        }
    }

    // Priority 3: Emergency healing via Holy Shock.
    if has_emergency_target(combatant.team, &ctx.combatants) {
        if try_holy_shock_heal(
            commands,
            combat_log,
            abilities,
            combatant,
            my_pos,
            auras,
            ctx,
            &mut builder,
        ) {
            builder.finish();
            return true;
        }
    } else {
        builder.reject(
            AbilityType::HolyShock,
            RejectionReason::PreconditionUnmet {
                note: "no ally below emergency HP threshold (heal mode)".into(),
            },
        );
    }

    // Priority 4: Hammer of Justice (rotation). Suppressed while the
    // reservation holds (living enemy healer + not PRESSURED) — the cooldown
    // is saved for dips (R8). Released under pressure so self-peel HoJ on
    // the Paladin's own attacker is never starved.
    if matches!(plan.hoj, HojPlan::Rotation) {
        if try_hammer_of_justice(
            commands,
            combat_log,
            abilities,
            entity,
            combatant,
            my_pos,
            auras,
            ctx,
            same_frame_cc_queue,
            &mut builder,
        ) {
            builder.finish();
            return true;
        }
    } else {
        builder.reject(
            AbilityType::HammerOfJustice,
            RejectionReason::PreconditionUnmet {
                note: "HoJ reserved for enemy-healer dip".into(),
            },
        );
    }

    // Priority 5: Flash of Light.
    if try_flash_of_light(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        plan.cast_defer,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 6: Holy Light.
    if try_holy_light(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        plan.cast_defer,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 7: Cleanse - Maintenance (team-healthy only).
    if ctx.is_team_healthy(HEALTHY_HP_THRESHOLD, my_pos)
        && try_cleanse(
            commands,
            combat_log,
            abilities,
            entity,
            combatant,
            my_pos,
            auras,
            ctx,
            50,
            &mut builder,
        )
    {
        builder.finish();
        return true;
    }

    // Priority 8: Holy Shock (damage) — team-healthy only.
    if ctx.is_team_healthy(HEALTHY_HP_THRESHOLD, my_pos) {
        if try_holy_shock_damage(
            commands,
            combat_log,
            abilities,
            combatant,
            my_pos,
            auras,
            ctx,
            &mut builder,
        ) {
            builder.finish();
            return true;
        }
    } else {
        builder.reject(
            AbilityType::HolyShock,
            RejectionReason::PreconditionUnmet {
                note: "team not healthy enough for Holy Shock damage".into(),
            },
        );
    }

    builder.finish();
    false
}

/// Try to activate Divine Shield from the normal dispatch path.
pub fn try_divine_shield(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    auras: Option<&ActiveAuras>,
    _ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::DivineShield;
    if abilities.get(&ability).is_none() {
        return false;
    }

    if combatant
        .ability_cooldowns
        .get(&ability)
        .copied()
        .unwrap_or(0.0)
        > 0.0
    {
        let remaining = combatant
            .ability_cooldowns
            .get(&ability)
            .copied()
            .unwrap_or(0.0);
        builder.reject(ability, RejectionReason::OnCooldown { remaining });
        return false;
    }

    if auras.is_some_and(|a| {
        a.auras
            .iter()
            .any(|aura| aura.effect_type == AuraType::DamageImmunity)
    }) {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }

    let self_hp_pct = if combatant.max_health > 0.0 {
        combatant.current_health / combatant.max_health
    } else {
        1.0
    };

    let survival_trigger = self_hp_pct < DIVINE_SHIELD_HP_THRESHOLD;
    let pressure_trigger = self_hp_pct < LOW_HP_THRESHOLD;

    if !survival_trigger && !pressure_trigger {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "self HP above defensive trigger thresholds".into(),
            },
        );
        return false;
    }

    builder.choose(ability, Some(entity), true);

    let caster_id = combatant_id(combatant.team, combatant.slot, combatant.class);
    info!("{} activates Divine Shield!", caster_id);

    commands.spawn(DivineShieldPending {
        caster: entity,
        caster_team: combatant.team,
        caster_slot: combatant.slot,
        caster_class: combatant.class,
    });

    combatant.start_cooldown(ability, abilities);
    combatant.global_cooldown = GCD;

    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Divine Shield",
        None,
        "casts",
    );

    true
}

/// Pure while-CC Divine Shield decision (fix seam): should the CC-break bubble
/// fire this frame?
///
/// - **Self trigger** (unchanged): self HP fraction below
///   `DIVINE_SHIELD_HP_THRESHOLD` — the Paladin bubbles to survive regardless
///   of the ally state or how much CC is left.
/// - **Teammate trigger** (widened): an ally is genuinely in danger
///   (`lowest_ally_hp_pct < LOW_HP_THRESHOLD`, was `< DIVINE_SHIELD_HP_THRESHOLD`)
///   AND enough incapacitation remains to buy real acting time
///   (`max_incap_remaining >= DIVINE_SHIELD_MIN_CC_REMAINING`). The bubble
///   purges the Paladin's own CC, so it is the fear-break tool — but a 5-minute
///   cooldown is only worth spending when the break actually lets the Paladin
///   heal for a meaningful window. `lowest_ally_hp_pct` excludes self (the self
///   trigger owns that) and is `f32::INFINITY` when no living teammate exists.
///
/// NOTE: widening the teammate trigger changes behavior on ALL maps (a feared
/// Paladin with a low ally happens on BasicArena too, not just PillaredArena) —
/// intended and accepted, like the melee tempo reset.
pub fn divine_shield_while_cc_should_fire(
    self_hp_pct: f32,
    lowest_ally_hp_pct: f32,
    max_incap_remaining: f32,
) -> bool {
    let self_in_danger = self_hp_pct < DIVINE_SHIELD_HP_THRESHOLD;
    let teammate_in_danger = lowest_ally_hp_pct < LOW_HP_THRESHOLD
        && max_incap_remaining >= DIVINE_SHIELD_MIN_CC_REMAINING;
    self_in_danger || teammate_in_danger
}

/// Try to use Divine Shield while incapacitated (CC break path).
///
/// Called from `combat_ai.rs` before the incapacitation gate. The caller owns
/// the builder lifecycle — it starts one for this Paladin (the regular dispatch
/// never runs this frame) and finishes after the call.
pub fn try_divine_shield_while_cc(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::DivineShield;
    if abilities.get(&ability).is_none() {
        return false;
    }

    if combatant
        .ability_cooldowns
        .get(&ability)
        .copied()
        .unwrap_or(0.0)
        > 0.0
    {
        let remaining = combatant
            .ability_cooldowns
            .get(&ability)
            .copied()
            .unwrap_or(0.0);
        builder.reject(ability, RejectionReason::OnCooldown { remaining });
        return false;
    }

    if auras.is_some_and(|a| {
        a.auras
            .iter()
            .any(|aura| aura.effect_type == AuraType::DamageImmunity)
    }) {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }

    // Lowest ally HP fraction, EXCLUDING self (the self trigger owns self HP).
    // f32::INFINITY when no living teammate exists (1v1 / last alive).
    let lowest_ally_hp_pct = ctx
        .combatants
        .values()
        .filter(|info| {
            info.team == combatant.team
                && info.entity != entity
                && info.current_health > 0.0
                && info.max_health > 0.0
                && !info.is_pet
        })
        .map(|info| info.current_health / info.max_health)
        .fold(f32::INFINITY, f32::min);

    // Max remaining incapacitation across self's cast-preventing CC auras — how
    // much acting time the bubble's CC-purge would actually free up.
    let max_incap_remaining = auras.map_or(0.0, |a| {
        a.auras
            .iter()
            .filter(|aura| crate::states::play_match::utils::is_incapacitating(&aura.effect_type))
            .map(|aura| aura.duration)
            .fold(0.0_f32, f32::max)
    });

    let self_hp_pct = if combatant.max_health > 0.0 {
        combatant.current_health / combatant.max_health
    } else {
        1.0
    };

    if !divine_shield_while_cc_should_fire(self_hp_pct, lowest_ally_hp_pct, max_incap_remaining) {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: format!(
                    "no self-critical HP, and no ally < {:.0}% with >= {:.1}s CC remaining \
                     (ally {:.0}%, {:.1}s left) — not worth burning Divine Shield while CC'd",
                    LOW_HP_THRESHOLD * 100.0,
                    DIVINE_SHIELD_MIN_CC_REMAINING,
                    lowest_ally_hp_pct * 100.0,
                    max_incap_remaining,
                ),
            },
        );
        return false;
    }

    builder.choose(ability, Some(entity), true);

    let caster_id = combatant_id(combatant.team, combatant.slot, combatant.class);
    info!("{} breaks CC with Divine Shield!", caster_id);

    commands.spawn(DivineShieldPending {
        caster: entity,
        caster_team: combatant.team,
        caster_slot: combatant.slot,
        caster_class: combatant.class,
    });

    combatant.start_cooldown(ability, abilities);
    combatant.global_cooldown = GCD;

    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Divine Shield",
        None,
        "casts",
    );

    true
}

/// Check if any ally is in an emergency situation (below critical HP threshold).
fn has_emergency_target(team: u8, combatant_info: &BTreeMap<Entity, CombatantInfo>) -> bool {
    combatant_info.values().any(|info| {
        info.team == team
            && !info.is_pet
            && info.current_health > 0.0
            && info.max_health > 0.0
            && (info.current_health / info.max_health) < CRITICAL_HP_THRESHOLD
    })
}

/// Try to cast Flash of Light on an injured ally.
///
/// Cast-vs-move urgency (R8, mirroring the Priest's R7 rule): while
/// `cast_defer` is `Some(threshold)` (a live ESCAPE window or DIP) and the
/// would-be heal target's HP fraction is ABOVE the threshold, the heal is
/// deferred — Flash of Light locks movement for its whole cast, which would
/// stall the walk/escape. At or below the threshold the dip has already
/// aborted (the teammate-HP abort fires at the same threshold BEFORE the
/// ability pass), so critical heals fire un-deferred.
fn try_flash_of_light(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    cast_defer: Option<f32>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::FlashOfLight;
    let def = abilities.get_unchecked(&ability);

    if combatant.current_mana < def.mana_cost {
        builder.reject(
            ability,
            RejectionReason::InsufficientMana {
                have: combatant.current_mana,
                need: def.mana_cost,
            },
        );
        return false;
    }

    // Reach (range, then sight) filters before health ranks: an occluded
    // lowest ally yields to the lowest one in sight.
    let target_info = match ctx.lowest_health_ally_in_reach(0.9, def.range, my_pos) {
        Ok(info) => info,
        Err(reason) => {
            builder.reject(
                ability,
                unreached_reason(
                    reason,
                    ability,
                    def,
                    combatant,
                    my_pos,
                    auras,
                    ctx,
                    PreCastOpts::default(),
                ),
            );
            return false;
        }
    };
    let target_entity = target_info.entity;
    let target_pos = target_info.position;

    if let Some(threshold) = cast_defer {
        if target_info.health_pct() > threshold {
            builder.reject(
                ability,
                RejectionReason::PreconditionUnmet {
                    note: "dip/escape/dispel/medic walk live: non-critical heal deferred"
                        .to_string(),
                },
            );
            return false;
        }
    }

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability,
                def,
                combatant,
                my_pos,
                auras,
                Some((target_entity, target_pos)),
                ctx,
                opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands
        .entity(entity)
        .insert(CastingState::new(ability, target_entity, cast_time));

    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        Some(target_info.log_id()),
        "begins casting",
    );

    true
}

/// Try to cast Holy Light on an injured ally between 50-85% HP.
///
/// Deferred while `cast_defer` is live and the target is above the urgency
/// threshold — Holy Light is the longest movement-locking cast the Paladin
/// has, and its target band (50-85% HP) sits above the urgency threshold by
/// construction, so a live window/dip always defers it.
fn try_holy_light(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    cast_defer: Option<f32>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::HolyLight;
    let def = abilities.get_unchecked(&ability);

    if combatant.current_mana < def.mana_cost {
        builder.reject(
            ability,
            RejectionReason::InsufficientMana {
                have: combatant.current_mana,
                need: def.mana_cost,
            },
        );
        return false;
    }

    let target_info =
        match ctx.lowest_health_ally_in_reach(SAFE_HEAL_MAX_THRESHOLD, def.range, my_pos) {
            Ok(info) => info,
            Err(reason) => {
                builder.reject(
                    ability,
                    unreached_reason(
                        reason,
                        ability,
                        def,
                        combatant,
                        my_pos,
                        auras,
                        ctx,
                        PreCastOpts::default(),
                    ),
                );
                return false;
            }
        };
    if let Some(threshold) = cast_defer {
        if target_info.health_pct() > threshold {
            builder.reject(
                ability,
                RejectionReason::PreconditionUnmet {
                    note: "dip/escape/dispel/medic walk live: non-critical heal deferred"
                        .to_string(),
                },
            );
            return false;
        }
    }
    if target_info.health_pct() < LOW_HP_THRESHOLD {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "target below LOW_HP — Flash of Light / Holy Shock should handle".into(),
            },
        );
        return false;
    }
    let target_entity = target_info.entity;
    let target_pos = target_info.position;

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((target_entity, target_pos)),
        ctx,
        opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability,
                def,
                combatant,
                my_pos,
                auras,
                Some((target_entity, target_pos)),
                ctx,
                opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands
        .entity(entity)
        .insert(CastingState::new(ability, target_entity, cast_time));

    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        Some(target_info.log_id()),
        "begins casting",
    );

    true
}

/// Try Holy Shock as a heal on an emergency target.
fn try_holy_shock_heal(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::HolyShock;
    let def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts::default();
    if !pre_cast_ok(ability, def, combatant, my_pos, auras, None, ctx, opts) {
        builder.reject(
            ability,
            classify_pre_cast_failure(ability, def, combatant, my_pos, auras, None, ctx, opts),
        );
        return false;
    }

    // Reach (range, then sight) filters before health ranks: an occluded
    // emergency yields to the lowest ally in sight, and no heal lands through a
    // pillar.
    let target_info = match ctx.lowest_health_ally_in_reach(LOW_HP_THRESHOLD, def.range, my_pos) {
        Ok(info) => info,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };
    let target_entity = target_info.entity;

    builder.choose(ability, Some(target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.start_cooldown(ability, abilities);

    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Holy Shock (Heal)",
        Some(target_info.log_id()),
        "casts",
    );

    commands.spawn(HolyShockHealPending {
        caster_spell_power: combatant.spell_power,
        caster_crit_chance: combatant.crit_chance,
        caster_team: combatant.team,
        caster_slot: combatant.slot,
        caster_class: combatant.class,
        target: target_entity,
    });

    true
}

/// Try Holy Shock as damage on an enemy.
fn try_holy_shock_damage(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::HolyShock;
    let def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts::default();
    if !pre_cast_ok(ability, def, combatant, my_pos, auras, None, ctx, opts) {
        builder.reject(
            ability,
            classify_pre_cast_failure(ability, def, combatant, my_pos, auras, None, ctx, opts),
        );
        return false;
    }

    // The first enemy the shock REACHES (range, then sight): an occluded enemy
    // in range yields to the next one in sight rather than being picked and
    // then refused.
    let mut occluded = false;
    let damage_target = ctx
        .combatants
        .iter()
        .filter(|(_, info)| info.team != combatant.team && info.current_health > 0.0)
        .filter(|(e, _)| !ctx.entity_is_immune(**e))
        .find_map(|(e, info)| {
            match super::cast_reach(ctx, HOLY_SHOCK_DAMAGE_RANGE, my_pos, info.position) {
                // Resolve the pet-aware id here — the filter above does NOT
                // exclude pets, so a Felhunter can be the target and its raw
                // (slot, class) would build an impossible "Team 2 Warlock #11".
                super::CastReach::Reaches => Some((e, info.position, info.log_id())),
                super::CastReach::LosBlocked => {
                    occluded = true;
                    None
                }
                super::CastReach::OutOfRange { .. } => None,
            }
        });

    let Some((target_entity, target_pos, target_id)) = damage_target else {
        builder.reject(
            ability,
            if occluded {
                RejectionReason::LosBlocked
            } else {
                RejectionReason::NoValidTarget
            },
        );
        return false;
    };

    let target_opts = PreCastOpts {
        check_friendly_cc: true,
        check_target_immune: true,
        ..Default::default()
    };
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((*target_entity, target_pos)),
        ctx,
        target_opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability,
                def,
                combatant,
                my_pos,
                auras,
                Some((*target_entity, target_pos)),
                ctx,
                target_opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(*target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.start_cooldown(ability, abilities);

    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        "Holy Shock (Damage)",
        Some(target_id),
        "casts",
    );

    commands.spawn(HolyShockDamagePending {
        caster_spell_power: combatant.spell_power,
        caster_crit_chance: combatant.crit_chance,
        caster_team: combatant.team,
        caster_slot: combatant.slot,
        caster_class: combatant.class,
        target: *target_entity,
        impact_from: (my_pos - target_pos).normalize_or_zero(),
    });

    true
}

/// Try Hammer of Justice on an enemy in melee range (the rotation path —
/// healer-preferring target selection among in-range eligible enemies).
fn try_hammer_of_justice(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::HammerOfJustice;
    let def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts::default();
    if !pre_cast_ok(ability, def, combatant, my_pos, auras, None, ctx, opts) {
        builder.reject(
            ability,
            classify_pre_cast_failure(ability, def, combatant, my_pos, auras, None, ctx, opts),
        );
        return false;
    }

    // Carry the pet-aware id (hoj_target_eligible already excludes pets, but the
    // id is resolved via log_id so this can't silently break if that changes);
    // class is kept only for the healer-preferring pick.
    let enemies_in_range: Vec<(&Entity, CharacterClass, crate::combat::log::CombatantId)> = ctx
        .combatants
        .iter()
        .filter(|(e, _)| hoj_target_eligible(ctx, combatant.team, **e))
        .filter_map(|(e, info)| {
            if my_pos.distance(info.position) <= def.range {
                Some((e, info.class, info.log_id()))
            } else {
                None
            }
        })
        .collect();

    let stun_target = enemies_in_range
        .iter()
        .find(|(_, class, _)| class.is_healer())
        .or_else(|| enemies_in_range.first())
        .cloned();

    let Some((target_entity, _target_class, target_id)) = stun_target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    cast_hammer_of_justice(
        commands,
        combat_log,
        entity,
        def,
        abilities,
        combatant,
        *target_entity,
        target_id,
        same_frame_cc_queue,
        builder,
    );

    true
}

/// Try Hammer of Justice on the DIP target (U8). Readiness re-runs the same
/// `pre_cast_ok` gate as the rotation; the arrival re-check covers
/// eligibility (dead/immune/DR-immune/stealthed) and range against the
/// specific dip target instead of the rotation's healer-preferring scan.
#[allow(clippy::too_many_arguments)]
fn try_dip_hammer_of_justice(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    target: Entity,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::HammerOfJustice;
    let def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts::default();
    if !pre_cast_ok(ability, def, combatant, my_pos, auras, None, ctx, opts) {
        builder.reject(
            ability,
            classify_pre_cast_failure(ability, def, combatant, my_pos, auras, None, ctx, opts),
        );
        return false;
    }

    if !hoj_target_eligible(ctx, combatant.team, target) {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    }
    let info = ctx
        .combatants
        .get(&target)
        .expect("eligible target is in snapshot");
    let distance = my_pos.distance(info.position);
    if distance > def.range {
        builder.reject(
            ability,
            RejectionReason::OutOfRange {
                distance,
                max: def.range,
            },
        );
        return false;
    }

    cast_hammer_of_justice(
        commands,
        combat_log,
        entity,
        def,
        abilities,
        combatant,
        target,
        info.log_id(),
        same_frame_cc_queue,
        builder,
    );

    true
}

/// Success-side Hammer of Justice bookkeeping shared by the rotation and the
/// dip cast: mana/GCD/cooldown, logging, the stun aura (pending + same-frame
/// CC queue), and the trace `choose`.
#[allow(clippy::too_many_arguments)]
fn cast_hammer_of_justice(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    entity: Entity,
    def: &AbilityConfig,
    abilities: &AbilityDefinitions,
    combatant: &mut Combatant,
    target_entity: Entity,
    target_id: crate::combat::log::CombatantId,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) {
    builder.choose(AbilityType::HammerOfJustice, Some(target_entity), true);

    // Caster-side gesture marker (A2). Hammer of Justice is instant AND
    // aura-only, so it enters neither generic caster hook and would otherwise
    // render nothing on the Paladin's side. Spawned on the committed-use branch
    // — the caster performed the gesture whether or not the stun stuck — and
    // unconditionally in BOTH modes, per
    // `cosmetic-marker-cross-mode-spawn-parity.md`; headless spawns it, never
    // reads it, and the match-exit `PlayMatchEntity` sweep reclaims it.
    //
    // Placed in the SHARED cast helper rather than in the two callers, so the
    // rotation HoJ and the dip HoJ both get it from one site and a future third
    // caller cannot forget it.
    commands.spawn((
        InstantAbilityFired {
            caster: entity,
            target: Some(target_entity),
            ability: AbilityType::HammerOfJustice,
            is_crit: false,
        },
        PlayMatchEntity,
    ));

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.start_cooldown(AbilityType::HammerOfJustice, abilities);

    let caster_id = combatant_id(combatant.team, combatant.slot, combatant.class);
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        Some(target_id.clone()),
        "casts",
    );

    if let Some(aura_def) = def.applies_aura.as_ref() {
        combat_log.log_crowd_control(
            caster_id.clone(),
            target_id.clone(),
            "Stun".to_string(),
            aura_def.duration,
            format!(
                "{}'s Hammer of Justice stuns {} ({:.1}s)",
                caster_id, target_id, aura_def.duration
            ),
        );
        let hoj_aura = Aura {
            effect_type: aura_def.aura_type,
            duration: aura_def.duration,
            magnitude: aura_def.magnitude,
            break_on_damage_threshold: aura_def.break_on_damage,
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: None,
            ability_name: def.name.to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: Some(def.spell_school),
            applied_this_frame: false,
            backlash_damage: None,
            dr_category_override: None,
            dispel_type: DispelType::Auto,
            compound: None,
            source_item: None,
            stacks: None,
            bloom: None,
        };
        same_frame_cc_queue.push((target_entity, hoj_aura.clone()));
        commands.spawn(AuraPending {
            target: target_entity,
            aura: hoj_aura,
        });
    }
}

/// Try Cleanse on an ally with a dispellable debuff.
fn try_cleanse(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    min_priority: i32,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    super::try_dispel_ally(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        min_priority,
        AbilityType::PaladinCleanse,
        "[CLEANSE]",
        "Cleanse",
        CharacterClass::Paladin,
        builder,
    )
}

/// Try to apply the Paladin's chosen aura to all allies.
fn try_paladin_aura(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    paladin_aura_this_frame: &mut std::collections::HashSet<Entity>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let (ability, aura_check_type, aura_name) = match combatant.paladin_aura {
        PaladinAura::DevotionAura => (
            AbilityType::DevotionAura,
            AuraType::DamageTakenReduction,
            "Devotion Aura",
        ),
        PaladinAura::ShadowResistanceAura => (
            AbilityType::ShadowResistanceAura,
            AuraType::SpellResistanceBuff,
            "Shadow Resistance Aura",
        ),
        PaladinAura::ConcentrationAura => (
            AbilityType::ConcentrationAura,
            AuraType::LockoutDurationReduction,
            "Concentration Aura",
        ),
    };

    let def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts::default();
    if !pre_cast_ok(ability, def, combatant, my_pos, auras, None, ctx, opts) {
        builder.reject(
            ability,
            classify_pre_cast_failure(ability, def, combatant, my_pos, auras, None, ctx, opts),
        );
        return false;
    }

    let has_aura = |e: &Entity| -> bool {
        ctx.active_auras
            .get(e)
            .map(|active| {
                active
                    .iter()
                    .any(|a| a.effect_type == aura_check_type && a.ability_name == aura_name)
            })
            .unwrap_or(false)
    };

    let allies: Vec<(&Entity, CharacterClass)> = ctx
        .combatants
        .iter()
        .filter(|(_, info)| {
            info.team == combatant.team && info.current_health > 0.0 && !info.is_pet
        })
        .map(|(e, info)| (e, info.class))
        .collect();

    if allies
        .iter()
        .any(|(e, _)| has_aura(e) || paladin_aura_this_frame.contains(*e))
    {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }

    let allies_to_buff: Vec<&Entity> = ctx
        .combatants
        .iter()
        .filter(|(_, info)| {
            info.team == combatant.team && info.current_health > 0.0 && !info.is_pet
        })
        .filter_map(|(e, info)| {
            if my_pos.distance(info.position) <= def.range && !paladin_aura_this_frame.contains(e) {
                Some(e)
            } else {
                None
            }
        })
        .collect();

    if allies_to_buff.is_empty() {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    }

    builder.choose(ability, None, true);

    combatant.global_cooldown = GCD;

    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        aura_name,
        None,
        "casts",
    );

    for ally_entity in allies_to_buff {
        paladin_aura_this_frame.insert(*ally_entity);
        if let Some(pending) = AuraPending::from_ability(*ally_entity, entity, def) {
            commands.spawn(pending);
        }
    }

    true
}

// ============================================================================
// Posture evaluation (healer movement AI — U8: FREE/PRESSURED/ESCAPE/DIP)
// ============================================================================
//
// Extracted to `paladin_postures.rs` (P1 — paladin.rs was >1k lines). The
// entry point and `dip_should_abort` are re-exported so the public
// `class_ai::paladin::` paths used by `combat_ai` and the probe suite hold.
pub use super::paladin_postures::{dip_should_abort, evaluate_paladin_posture};

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // Fix 2: while-CC Divine Shield trigger widening
    // ------------------------------------------------------------------------
    //
    // The bubble purges the Paladin's own CC, so the while-CC teammate trigger
    // is the fear-break tool. It was previously gated on an ally below the 30%
    // survival threshold (too late — the fear was over before the ally crossed
    // it). Widened to LOW_HP_THRESHOLD (50%) AND a meaningful chunk of CC still
    // remaining, so the 5-minute cooldown only burns when the break buys real
    // acting time.

    /// Feared Paladin (3s remaining), ally at 45% → the teammate trigger fires.
    #[test]
    fn while_cc_teammate_fires_when_ally_low_and_cc_remains() {
        assert!(
            divine_shield_while_cc_should_fire(0.9, 0.45, 3.0),
            "ally < 50% with 3s CC remaining should fire the CC-break bubble"
        );
    }

    /// Ally at 60% (not genuinely in danger) → rejected even with CC remaining.
    #[test]
    fn while_cc_teammate_rejected_when_ally_not_low() {
        assert!(
            !divine_shield_while_cc_should_fire(0.9, 0.60, 3.0),
            "ally above LOW_HP_THRESHOLD is not in danger — do not burn Divine Shield"
        );
    }

    /// Ally at 45% but only 1s of CC left → rejected: the break wouldn't buy
    /// meaningful acting time (below DIVINE_SHIELD_MIN_CC_REMAINING = 2.0s).
    #[test]
    fn while_cc_teammate_rejected_when_cc_nearly_over() {
        assert!(
            !divine_shield_while_cc_should_fire(0.9, 0.45, 1.0),
            "1s of CC left is not worth a 5-minute cooldown"
        );
        // Exactly at the 2.0s floor fires (>= boundary).
        assert!(
            divine_shield_while_cc_should_fire(0.9, 0.45, DIVINE_SHIELD_MIN_CC_REMAINING),
            "exactly the minimum CC-remaining floor should fire"
        );
    }

    /// The self-HP trigger is unchanged and independent of ally state: a
    /// self-critical Paladin bubbles regardless of ally HP or remaining CC.
    #[test]
    fn while_cc_self_trigger_fires_regardless_of_ally() {
        // Self critical, ally full, no CC remaining → still fires (self path).
        assert!(
            divine_shield_while_cc_should_fire(0.2, 1.0, 0.0),
            "self below the survival threshold fires regardless of ally/CC state"
        );
        // Self healthy, no living teammate (INFINITY), no CC → does not fire.
        assert!(
            !divine_shield_while_cc_should_fire(0.9, f32::INFINITY, 0.0),
            "healthy Paladin, no endangered ally → no fire"
        );
    }

    // ------------------------------------------------------------------------
    // AS-192: the Holy Shock heal needs sight of its ally
    // ------------------------------------------------------------------------

    /// The Holy Shock heal chooses among allies it can SEE. A Warrior at 20%
    /// behind a pillar and a Mage at 40% in sight, both in range: without the
    /// pillar the Warrior (lowest) is healed; with it, the Mage in sight is —
    /// no heal through the pillar, and no refusal while a sighted emergency
    /// waits. With the Warrior the only emergency, the heal is refused and
    /// traced `LosBlocked`.
    #[test]
    fn holy_shock_heal_needs_sight_of_its_ally() {
        use crate::states::play_match::decision_trace::ActorView;
        use crate::states::play_match::map_geometry::ObstacleVolume;
        use bevy::ecs::world::CommandQueue;

        let unit = |entity, class, position, health| CombatantInfo {
            entity,
            team: 1,
            slot: 0,
            class,
            current_health: health,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position,
            velocity: Vec3::ZERO,
            is_alive: true,
            stealthed: false,
            target: None,
            is_pet: false,
            casting_ability: None,
            pet_type: None,
            pet: None,
        };
        let (paladin, warrior, mage) = (
            Entity::from_raw(1),
            Entity::from_raw(2),
            Entity::from_raw(3),
        );
        let paladin_pos = Vec3::new(0.0, 1.0, 0.0);
        let roster_with = |mage_health| -> BTreeMap<_, _> {
            [
                (
                    paladin,
                    unit(paladin, CharacterClass::Paladin, paladin_pos, 100.0),
                ),
                (
                    warrior,
                    unit(
                        warrior,
                        CharacterClass::Warrior,
                        Vec3::new(16.0, 1.0, 0.0),
                        20.0,
                    ),
                ),
                (
                    mage,
                    unit(
                        mage,
                        CharacterClass::Mage,
                        Vec3::new(0.0, 1.0, 16.0),
                        mage_health,
                    ),
                ),
            ]
            .into_iter()
            .collect()
        };
        let pillar = [ObstacleVolume::Cylinder {
            center_xz: Vec2::new(8.0, 0.0),
            radius: 2.0,
            base_y: 0.0,
            height: 10.0,
        }];
        let abilities = AbilityDefinitions::default();

        // (healed ally, the HolyShock candidate)
        let shock = |roster: &BTreeMap<Entity, CombatantInfo>, obstacles: &[ObstacleVolume]| {
            let (auras, dr, cds) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
            let ctx = CombatContext::new(
                paladin,
                1,
                roster,
                &auras,
                &dr,
                &cds,
                obstacles,
                Default::default(),
                Default::default(),
            );
            let mut world = World::new();
            let mut queue = CommandQueue::default();
            let mut combatant = Combatant::new(1, 0, CharacterClass::Paladin);
            let mut trace = DecisionTrace::default();
            {
                let mut commands = Commands::new(&mut queue, &world);
                let mut builder =
                    trace.start_ability_decision(ActorView::from_info(&roster[&paladin]), None);
                try_holy_shock_heal(
                    &mut commands,
                    &mut CombatLog::default(),
                    &abilities,
                    &mut combatant,
                    paladin_pos,
                    None,
                    &ctx,
                    &mut builder,
                );
                builder.finish();
            }
            queue.apply(&mut world);
            let healed = world
                .query::<&HolyShockHealPending>()
                .iter(&world)
                .map(|pending| pending.target)
                .next();
            let event = serde_json::to_value(&trace.pending_events[0]).unwrap();
            let candidate = event["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["ability"] == "HolyShock")
                .cloned()
                .unwrap();
            (healed, candidate)
        };

        let both = roster_with(40.0);
        assert_eq!(
            shock(&both, &[]).0,
            Some(warrior),
            "in sight: the lowest ally"
        );
        let (healed, candidate) = shock(&both, &pillar);
        assert_eq!(
            healed,
            Some(mage),
            "the occluded Warrior yields to the Mage in sight"
        );
        assert_eq!(candidate["status"], "chosen");

        let warrior_only = roster_with(100.0);
        assert_eq!(shock(&warrior_only, &[]).0, Some(warrior));
        let (healed, candidate) = shock(&warrior_only, &pillar);
        assert_eq!(healed, None, "no Holy Shock heal through the pillar");
        assert_eq!(candidate["status"], "rejected");
        assert_eq!(candidate["reason"], "LosBlocked");
    }
}

// ----------------------------------------------------------------------------
// AS-202: the Paladin's heals and Holy Shock damage choose targets in sight
// ----------------------------------------------------------------------------

#[cfg(test)]
mod reach_tests {
    use super::*;
    use crate::states::play_match::class_ai::reach_fixture::*;
    use crate::states::play_match::map_geometry::ObstacleVolume;
    use CharacterClass::{Mage, Paladin, Warrior};

    /// The Paladin with a Warrior `BEHIND` the pillar and a Mage `IN_SIGHT`,
    /// both on `team` (1: allies, 2: enemies).
    fn scene(team: u8, warrior_hp: f32, mage_hp: f32) -> ReachScene {
        ReachScene::new(
            Paladin,
            &[
                (team, Warrior, BEHIND, warrior_hp),
                (team, Mage, IN_SIGHT, mage_hp),
            ],
        )
    }

    fn flash_of_light(s: &mut ReachScene, obstacles: &[ObstacleVolume]) -> DecisionTrace {
        let abilities = AbilityDefinitions::default();
        let me = s.units[0];
        s.run(obstacles, |commands, ctx, combatant, builder| {
            try_flash_of_light(
                commands,
                &mut CombatLog::default(),
                &abilities,
                me,
                combatant,
                CASTER,
                None,
                ctx,
                None,
                builder,
            )
        })
    }

    fn holy_light(s: &mut ReachScene, obstacles: &[ObstacleVolume]) -> DecisionTrace {
        let abilities = AbilityDefinitions::default();
        let me = s.units[0];
        s.run(obstacles, |commands, ctx, combatant, builder| {
            try_holy_light(
                commands,
                &mut CombatLog::default(),
                &abilities,
                me,
                combatant,
                CASTER,
                None,
                ctx,
                None,
                builder,
            )
        })
    }

    fn holy_shock_damage(s: &mut ReachScene, obstacles: &[ObstacleVolume]) -> DecisionTrace {
        let abilities = AbilityDefinitions::default();
        s.run(obstacles, |commands, ctx, combatant, builder| {
            try_holy_shock_damage(
                commands,
                &mut CombatLog::default(),
                &abilities,
                combatant,
                CASTER,
                None,
                ctx,
                builder,
            )
        })
    }

    /// Flash of Light heals the lowest ally it can SEE.
    #[test]
    fn flash_of_light_falls_back_to_the_lowest_ally_in_sight() {
        let mut s = scene(1, 0.3, 0.6);
        let (warrior, mage) = (s.units[1], s.units[2]);
        assert_eq!(
            outcome(&flash_of_light(&mut s, &[])),
            chose("FlashOfLight", warrior),
            "no pillar: the lowest ally"
        );
        assert_eq!(
            outcome(&flash_of_light(&mut s, &pillar())),
            chose("FlashOfLight", mage),
            "the occluded Warrior yields to the Mage in sight"
        );

        let mut s = scene(1, 0.6, 0.3);
        let mage = s.units[2];
        assert_eq!(
            outcome(&flash_of_light(&mut s, &pillar())),
            chose("FlashOfLight", mage),
            "a lowest ally in sight is healed with the pillar standing"
        );

        let mut s = scene(1, 0.3, 1.0);
        let trace = flash_of_light(&mut s, &pillar());
        assert_eq!(
            outcome(&trace),
            None,
            "no Flash of Light through the pillar"
        );
        assert_eq!(candidate(&trace, "FlashOfLight")["reason"], "LosBlocked");
    }

    /// Holy Light heals the lowest ally in its band (50-85%) it can SEE.
    #[test]
    fn holy_light_falls_back_to_the_lowest_ally_in_sight() {
        let mut s = scene(1, 0.6, 0.75);
        let (warrior, mage) = (s.units[1], s.units[2]);
        assert_eq!(
            outcome(&holy_light(&mut s, &[])),
            chose("HolyLight", warrior),
            "no pillar: the lowest ally"
        );
        assert_eq!(
            outcome(&holy_light(&mut s, &pillar())),
            chose("HolyLight", mage),
            "the occluded Warrior yields to the Mage in sight"
        );

        let mut s = scene(1, 0.75, 0.6);
        let mage = s.units[2];
        assert_eq!(
            outcome(&holy_light(&mut s, &pillar())),
            chose("HolyLight", mage),
            "a lowest ally in sight is healed with the pillar standing"
        );

        let mut s = scene(1, 0.6, 1.0);
        let trace = holy_light(&mut s, &pillar());
        assert_eq!(outcome(&trace), None, "no Holy Light through the pillar");
        assert_eq!(candidate(&trace, "HolyLight")["reason"], "LosBlocked");
    }

    /// Holy Shock damage strikes the first enemy within 20yd it can SEE. The
    /// Warrior precedes the Mage in entity order, so it is the first pick
    /// whenever it is in sight.
    #[test]
    fn holy_shock_damage_falls_back_to_an_enemy_in_sight() {
        let mut s = scene(2, 1.0, 1.0);
        let (warrior, mage) = (s.units[1], s.units[2]);
        assert_eq!(
            outcome(&holy_shock_damage(&mut s, &[])),
            chose("HolyShock", warrior),
            "no pillar: the first enemy in range"
        );
        assert_eq!(
            outcome(&holy_shock_damage(&mut s, &pillar())),
            chose("HolyShock", mage),
            "the occluded Warrior yields to the Mage in sight"
        );

        // The Mage alone, in sight with the pillar standing.
        s.roster.remove(&warrior);
        assert_eq!(
            outcome(&holy_shock_damage(&mut s, &pillar())),
            chose("HolyShock", mage),
            "a first enemy in sight is struck with the pillar standing"
        );

        let mut s = scene(2, 1.0, 1.0);
        let mage = s.units[2];
        s.roster.remove(&mage);
        let trace = holy_shock_damage(&mut s, &pillar());
        assert_eq!(outcome(&trace), None, "no Holy Shock through the pillar");
        assert_eq!(candidate(&trace, "HolyShock")["reason"], "LosBlocked");
    }

    /// A silenced Paladin is refused Flash of Light for the silence, whether
    /// its only hurt ally is in sight or behind the pillar: sight cannot hide
    /// the caster's own state (AS-204).
    #[test]
    fn an_occluded_heal_still_reports_the_paladin_silenced() {
        let silenced = ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::Silence,
                ..Default::default()
            }],
        };
        let reason = |obstacles: &[ObstacleVolume]| {
            let abilities = AbilityDefinitions::default();
            let mut s = scene(1, 0.3, 1.0);
            let me = s.units[0];
            let trace = s.run(obstacles, |commands, ctx, combatant, builder| {
                try_flash_of_light(
                    commands,
                    &mut CombatLog::default(),
                    &abilities,
                    me,
                    combatant,
                    CASTER,
                    Some(&silenced),
                    ctx,
                    None,
                    builder,
                )
            });
            candidate(&trace, "FlashOfLight")["reason"].clone()
        };
        let open = reason(&[]);
        assert!(open.get("SilencedOrLocked").is_some(), "{open}");
        assert_eq!(reason(&pillar()), open, "behind the pillar");
    }

    /// The same for Holy Light: a silenced Paladin whose only ally in Holy
    /// Light's band is behind the pillar is refused for the silence.
    #[test]
    fn an_occluded_holy_light_still_reports_the_paladin_silenced() {
        let silenced = ActiveAuras {
            auras: vec![Aura {
                effect_type: AuraType::Silence,
                ..Default::default()
            }],
        };
        let reason = |obstacles: &[ObstacleVolume]| {
            let abilities = AbilityDefinitions::default();
            let mut s = scene(1, 0.6, 1.0);
            let me = s.units[0];
            let trace = s.run(obstacles, |commands, ctx, combatant, builder| {
                try_holy_light(
                    commands,
                    &mut CombatLog::default(),
                    &abilities,
                    me,
                    combatant,
                    CASTER,
                    Some(&silenced),
                    ctx,
                    None,
                    builder,
                )
            });
            candidate(&trace, "HolyLight")["reason"].clone()
        };
        let open = reason(&[]);
        assert!(open.get("SilencedOrLocked").is_some(), "{open}");
        assert_eq!(reason(&pillar()), open, "behind the pillar");
    }
}
