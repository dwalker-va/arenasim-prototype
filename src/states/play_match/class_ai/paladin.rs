//! Paladin AI Module
//!
//! Holy warrior and healer - combines healing with melee utility.
//!
//! ## Priority Order
//! 1. Paladin Aura (buff all allies pre-combat — Devotion/Shadow Resistance/Concentration)
//! 1.5. Divine Shield (emergency: self < 30% HP, or CC break for teammate)
//! 2. Cleanse - Urgent (Polymorph, Fear on allies)
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
use crate::states::play_match::combat_core::{
    calculate_cast_time, compass_directions_16, score_directions, AnchorConstraint, ScorerInputs,
};
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{
    CRITICAL_HP_THRESHOLD, DIVINE_SHIELD_HP_THRESHOLD, GCD, HEALTHY_HP_THRESHOLD,
    HOLY_SHOCK_DAMAGE_RANGE, LOW_HP_THRESHOLD, SAFE_HEAL_MAX_THRESHOLD,
};
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, MovementGoalKind, MovementTrigger,
    Posture as TracePosture, RejectionReason,
};
use crate::states::play_match::movement_config::{MovementConfig, SharedMovementConfig};
use crate::states::play_match::utils::{combatant_id, log_ability_use};

use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};
use super::healer_postures::{
    compound_pressure_trigger, escape_tick, select_sticky_anchor, start_movement_event,
    start_movement_event_with_target, SCORER_LOOKAHEAD,
};
use super::priest::escape_window;

use super::{CombatContext, CombatantInfo};

/// Per-tick output of [`evaluate_paladin_posture`], threaded into
/// [`decide_paladin_action`] (mirrors the Priest's `escape_defer` but adds
/// the Hammer of Justice gate).
pub struct PaladinMovementPlan {
    /// `Some(urgency_hp_threshold)` while an ESCAPE window OR a DIP is live:
    /// the heal ladder defers non-critical movement-locking casts (Flash of
    /// Light, Holy Light) whose would-be target is ABOVE the threshold —
    /// casting locks movement, and an undeferred heal mid-dip would stall
    /// the walk into a budget abort (R8; same rule as the Priest's R7).
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
/// rotation's target scan applies (alive enemy non-pet, not stealthed, not
/// damage-immune, not stun-DR-immune). Shared by the rotation, the DIP entry
/// predicate, and the DIP arrival/abort re-checks so the dip can never walk
/// toward a guaranteed-rejected cast (R8).
pub fn hoj_target_eligible(ctx: &CombatContext, my_team: u8, target: Entity) -> bool {
    let Some(info) = ctx.combatants.get(&target) else {
        return false;
    };
    info.team != my_team
        && info.current_health > 0.0
        && !info.stealthed
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

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, combatant.target, my_pos) else {
        return false;
    };

    // Priority 1: Paladin Aura.
    if try_paladin_aura(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        paladin_aura_this_frame, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 1.5: Divine Shield (emergency defensive).
    if try_divine_shield(
        commands, combat_log, abilities, entity, combatant, auras, ctx, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 2: Cleanse - Urgent (Polymorph, Fear).
    if try_cleanse(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        90, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 2.5: DIP Hammer of Justice (U8). The dip walked up to
    // dip_budget seconds for exactly this cast — it outranks everything
    // below the urgent dispel. On success the posture returns to FREE
    // (DipComplete) and the walk directive dies with it; the return to the
    // kill target happens naturally via FREE's legacy melee pursuit.
    if let HojPlan::DipCast { target, completed_state } = &plan.hoj {
        if try_dip_hammer_of_justice(
            commands, combat_log, abilities, combatant, my_pos, auras, ctx,
            *target, same_frame_cc_queue, &mut builder,
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
    if has_emergency_target(combatant.team, ctx.combatants) {
        if try_holy_shock_heal(
            commands, combat_log, abilities, combatant, my_pos, auras, ctx, &mut builder,
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
            commands, combat_log, abilities, combatant, my_pos, auras, ctx,
            same_frame_cc_queue, &mut builder,
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
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        plan.cast_defer, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 6: Holy Light.
    if try_holy_light(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        plan.cast_defer, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 7: Cleanse - Maintenance (team-healthy only).
    if ctx.is_team_healthy(HEALTHY_HP_THRESHOLD, my_pos) {
        if try_cleanse(
            commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
            50, &mut builder,
        ) {
            builder.finish();
            return true;
        }
    }

    // Priority 8: Holy Shock (damage) — team-healthy only.
    if ctx.is_team_healthy(HEALTHY_HP_THRESHOLD, my_pos) {
        if try_holy_shock_damage(
            commands, combat_log, abilities, combatant, my_pos, auras, ctx, &mut builder,
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
    let def = match abilities.get(&ability) {
        Some(d) => d,
        None => return false,
    };

    if combatant.ability_cooldowns.get(&ability).copied().unwrap_or(0.0) > 0.0 {
        let remaining = combatant.ability_cooldowns.get(&ability).copied().unwrap_or(0.0);
        builder.reject(ability, RejectionReason::OnCooldown { remaining });
        return false;
    }

    if auras.map_or(false, |a| a.auras.iter().any(|aura| aura.effect_type == AuraType::DamageImmunity)) {
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

    let caster_id = combatant_id(combatant.team, combatant.class);
    info!("{} activates Divine Shield!", caster_id);

    commands.spawn(DivineShieldPending {
        caster: entity,
        caster_team: combatant.team,
        caster_class: combatant.class,
    });

    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Divine Shield", None, "casts");

    true
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
    let def = match abilities.get(&ability) {
        Some(d) => d,
        None => return false,
    };

    if combatant.ability_cooldowns.get(&ability).copied().unwrap_or(0.0) > 0.0 {
        let remaining = combatant.ability_cooldowns.get(&ability).copied().unwrap_or(0.0);
        builder.reject(ability, RejectionReason::OnCooldown { remaining });
        return false;
    }

    if auras.map_or(false, |a| a.auras.iter().any(|aura| aura.effect_type == AuraType::DamageImmunity)) {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }

    let teammate_in_danger = ctx.combatants.values().any(|info| {
        info.team == combatant.team
            && info.current_health > 0.0
            && info.max_health > 0.0
            && !info.is_pet
            && (info.current_health / info.max_health) < DIVINE_SHIELD_HP_THRESHOLD
    });

    let self_hp_pct = if combatant.max_health > 0.0 {
        combatant.current_health / combatant.max_health
    } else {
        1.0
    };
    let self_in_danger = self_hp_pct < DIVINE_SHIELD_HP_THRESHOLD;

    if !teammate_in_danger && !self_in_danger {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "no teammate (or self) in critical HP — not worth burning Divine Shield while CC'd".into(),
            },
        );
        return false;
    }

    builder.choose(ability, Some(entity), true);

    let caster_id = combatant_id(combatant.team, combatant.class);
    info!("{} breaks CC with Divine Shield!", caster_id);

    commands.spawn(DivineShieldPending {
        caster: entity,
        caster_team: combatant.team,
        caster_class: combatant.class,
    });

    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Divine Shield", None, "casts");

    true
}

/// Check if any ally is in an emergency situation (below critical HP threshold).
fn has_emergency_target(
    team: u8,
    combatant_info: &BTreeMap<Entity, CombatantInfo>,
) -> bool {
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

    let Some(target_info) = ctx.lowest_health_ally_below(0.9, def.range, my_pos) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let target_entity = target_info.entity;
    let target_class = target_info.class;
    let target_pos = target_info.position;

    if let Some(threshold) = cast_defer {
        if target_info.health_pct() > threshold {
            builder.reject(
                ability,
                RejectionReason::PreconditionUnmet {
                    note: "dip/escape live: non-critical heal deferred".to_string(),
                },
            );
            return false;
        }
    }

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        ability, def, combatant, my_pos, auras,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, auras,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(ability, target_entity, cast_time));

    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, Some((combatant.team, target_class)), "begins casting");

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

    let Some(target_info) = ctx.lowest_health_ally_below(SAFE_HEAL_MAX_THRESHOLD, def.range, my_pos) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    if let Some(threshold) = cast_defer {
        if target_info.health_pct() > threshold {
            builder.reject(
                ability,
                RejectionReason::PreconditionUnmet {
                    note: "dip/escape live: non-critical heal deferred".to_string(),
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
    let target_class = target_info.class;
    let target_pos = target_info.position;

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        ability, def, combatant, my_pos, auras,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, auras,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(ability, target_entity, cast_time));

    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, Some((combatant.team, target_class)), "begins casting");

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

    let Some(target_info) = ctx.lowest_health_ally_below(LOW_HP_THRESHOLD, def.range, my_pos) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let target_entity = target_info.entity;
    let target_class = target_info.class;

    builder.choose(ability, Some(target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    log_ability_use(combat_log, combatant.team, combatant.class, "Holy Shock (Heal)", Some((combatant.team, target_class)), "casts");

    commands.spawn(HolyShockHealPending {
        caster_spell_power: combatant.spell_power,
        caster_crit_chance: combatant.crit_chance,
        caster_team: combatant.team,
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

    let damage_target = ctx.combatants
        .iter()
        .filter(|(_, info)| {
            info.team != combatant.team && info.current_health > 0.0 && !info.stealthed
        })
        .filter(|(e, _)| !ctx.entity_is_immune(**e))
        .find_map(|(e, info)| {
            if my_pos.distance(info.position) <= HOLY_SHOCK_DAMAGE_RANGE {
                Some((e, info.position, info.class))
            } else {
                None
            }
        });

    let Some((target_entity, target_pos, target_class)) = damage_target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    let target_opts = PreCastOpts {
        check_friendly_cc: true,
        check_target_immune: true,
        ..Default::default()
    };
    if !pre_cast_ok(
        ability, def, combatant, my_pos, auras,
        Some((*target_entity, target_pos)), ctx, target_opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, auras,
                Some((*target_entity, target_pos)), ctx, target_opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(*target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

    let enemy_team = if combatant.team == 1 { 2 } else { 1 };
    log_ability_use(combat_log, combatant.team, combatant.class, "Holy Shock (Damage)", Some((enemy_team, target_class)), "casts");

    commands.spawn(HolyShockDamagePending {
        caster_spell_power: combatant.spell_power,
        caster_crit_chance: combatant.crit_chance,
        caster_team: combatant.team,
        caster_class: combatant.class,
        target: *target_entity,
    });

    true
}

/// Try Hammer of Justice on an enemy in melee range (the rotation path —
/// healer-preferring target selection among in-range eligible enemies).
fn try_hammer_of_justice(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
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

    let enemies_in_range: Vec<(&Entity, CharacterClass)> = ctx.combatants
        .iter()
        .filter(|(e, _)| hoj_target_eligible(ctx, combatant.team, **e))
        .filter_map(|(e, info)| {
            if my_pos.distance(info.position) <= def.range {
                Some((e, info.class))
            } else {
                None
            }
        })
        .collect();

    let stun_target = enemies_in_range
        .iter()
        .find(|(_, class)| class.is_healer())
        .or_else(|| enemies_in_range.first())
        .copied();

    let Some((target_entity, target_class)) = stun_target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    cast_hammer_of_justice(
        commands, combat_log, def, combatant, *target_entity, target_class,
        same_frame_cc_queue, builder,
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
    let info = ctx.combatants.get(&target).expect("eligible target is in snapshot");
    let distance = my_pos.distance(info.position);
    if distance > def.range {
        builder.reject(ability, RejectionReason::OutOfRange { distance, max: def.range });
        return false;
    }

    cast_hammer_of_justice(
        commands, combat_log, def, combatant, target, info.class,
        same_frame_cc_queue, builder,
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
    def: &AbilityConfig,
    combatant: &mut Combatant,
    target_entity: Entity,
    target_class: CharacterClass,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) {
    builder.choose(AbilityType::HammerOfJustice, Some(target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(AbilityType::HammerOfJustice, def.cooldown);

    let caster_id = combatant_id(combatant.team, combatant.class);
    let enemy_team = if combatant.team == 1 { 2 } else { 1 };
    let target_id = format!("Team {} {}", enemy_team, target_class.name());
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, Some((enemy_team, target_class)), "casts");

    if let Some(aura_def) = def.applies_aura.as_ref() {
        combat_log.log_crowd_control(
            caster_id,
            target_id.clone(),
            "Stun".to_string(),
            aura_def.duration,
            format!(
                "Team {} {}'s Hammer of Justice stuns {} ({:.1}s)",
                combatant.team,
                combatant.class.name(),
                target_id,
                aura_def.duration
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
                active.iter().any(|a| {
                    a.effect_type == aura_check_type
                        && a.ability_name == aura_name
                })
            })
            .unwrap_or(false)
    };

    let allies: Vec<(&Entity, CharacterClass)> = ctx.combatants
        .iter()
        .filter(|(_, info)| info.team == combatant.team && info.current_health > 0.0 && !info.is_pet)
        .map(|(e, info)| (e, info.class))
        .collect();

    if allies.iter().any(|(e, _)| has_aura(e) || paladin_aura_this_frame.contains(*e)) {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }

    let allies_to_buff: Vec<&Entity> = ctx.combatants
        .iter()
        .filter(|(_, info)| info.team == combatant.team && info.current_health > 0.0 && !info.is_pet)
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

    log_ability_use(combat_log, combatant.team, combatant.class, aura_name, None, "casts");

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

/// Evaluate the Paladin's movement posture and issue/refresh a
/// [`MovementDirective`] accordingly. Runs at the top of the Paladin's decide
/// tick (mirroring `evaluate_priest_posture`): BEFORE the GCD short-circuit,
/// only after gates open, never for casting Paladins (R12 is structural —
/// `decide_abilities` excludes `CastingState` entities and `move_to_target`
/// blocks directive execution while casting).
///
/// The Paladin state machine differs from the Priest's in three ways (R8):
///
/// - **FREE keeps the melee identity.** No formation point, no directive —
///   the legacy `preferred_range 2.0` pursuit governs. The only FREE-side
///   behavior is the DIP entry check.
/// - **PRESSURED adds the healing-heavy trigger**: the Priest compound
///   trigger (focused) OR the lowest HP fraction across living non-pet team
///   members (self included) below `paladin.healing_heavy_hp`. Movement
///   retreats from threats toward `paladin.fallback_range` (band-hold: once
///   every threat is at/beyond the band, the Paladin stands and heals)
///   while staying within heal range of the anchor ally.
/// - **DIP (FREE → DIP only)**: a committed walk to the enemy healer for
///   Hammer of Justice. Entry requires HoJ ready (same `pre_cast_ok` gate
///   as the rotation), a stable teammate (anchor ally above the urgency HP
///   threshold and not CC'd — vacuously stable with no living teammate, so
///   1v1 Paladin-vs-healer still dips), and an eligible enemy healer within
///   `HoJ range + dip_budget × effective speed`. The dip aborts (→ FREE,
///   `DipAbort`) on teammate HP dive (AE3 — without casting), target
///   dead/immune/DR-immune/stealthed, or budget expiry; becoming focused
///   preempts unconditionally (→ PRESSURED, `PressuredEnter` — never
///   `DipAbort`). When the Paladin's kill target IS the enemy healer and is
///   already within HoJ range, the dip still runs as a zero-length dip:
///   `DipEnter` and `DipComplete` fire on the same decide tick and the cast
///   goes through the dip path, keeping every unpressured HoJ-on-healer
///   attributable to a dip in the trace (the documented choice for the
///   plan's deferred zero-length-dip question).
///
/// Returns the [`PaladinMovementPlan`] for this tick: `cast_defer` is
/// `Some(urgency_hp_threshold)` while an ESCAPE window or DIP is live (the
/// heal ladder defers non-critical movement-locking casts), and `hoj` gates
/// rotation Hammer of Justice (reservation while a living enemy healer
/// exists and the Paladin is unpressured; `DipCast` on dip arrival).
#[allow(clippy::too_many_arguments)]
pub fn evaluate_paladin_posture(
    commands: &mut Commands,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    posture: Option<&mut HealerPosture>,
    directive: Option<&MovementDirective>,
    movement: &MovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
) -> PaladinMovementPlan {
    // First evaluation inserts the persistent component via Commands
    // (visible to this tick's executor through the existing apply_deferred,
    // and to next tick's query).
    let mut local = HealerPosture::new(now);
    let needs_insert = posture.is_none();
    let state: &mut HealerPosture = match posture {
        Some(p) => p,
        None => &mut local,
    };

    let shared = &movement.shared;
    let pal = &movement.paladin;

    // --- PRESSURED triggers (R8) ---
    // Focused: the Priest's compound trigger (R6), shared verbatim.
    let focused = compound_pressure_trigger(entity, my_pos, ctx, shared);
    // Healing-heavy (R8, observable + deterministic): a living non-pet team
    // member (self included) is below `healing_heavy_hp` AND a melee/pet
    // enemy is inside the danger radius. The melee-pressure conjunct is
    // load-bearing — without it a Paladin whose teammate routinely dips to
    // ~0.4 in a normal melee scrum would retreat permanently, deleting its
    // melee identity in matchups with NO enemy healer (the U2 identity
    // probe caught exactly this). With the conjunct, healing-heavy fires
    // only when the team is hurting AND a melee is in the Paladin's face —
    // the situation a retreat-to-heal actually helps.
    let melee_pressure = ctx
        .visible_enemies_within(entity, my_pos, shared.danger_radius)
        .iter()
        .any(|t| t.is_pet || t.class.is_melee());
    let team_hurting = ctx
        .alive_allies()
        .iter()
        .any(|a| a.health_pct() < pal.healing_heavy_hp);
    let healing_heavy = team_hurting && melee_pressure;
    // Degenerate-case gate (the Priest's R5 no-ally rule, applied to the
    // Paladin's retreat): PRESSURED exists to protect the team's healing
    // capacity — fall back, keep the team alive from safety. With no living
    // non-pet ally there is no team to retreat FOR, and falling back only
    // deletes the Paladin's melee output (it can heal itself from anywhere).
    // Validation caught the failure: every Paladin 1v1 collapsed (e.g. the
    // Paladin permanently kiting a Hunter's pet into a 300s draw, 85
    // PressuredEnter/Exit strobes per match). Melee identity governs when
    // alone; dips and rotation HoJ still apply (`alive_allies` includes
    // self, so require an ally other than us).
    let has_teammate = ctx.alive_allies().iter().any(|a| a.entity != entity);
    let trigger = (focused || healing_heavy) && has_teammate;

    let prev = state.posture;

    // --- ESCAPE entry window (R7 machinery, reused) ---
    let escape_window_secs = if prev == Posture::Pressured && trigger {
        let cc_remaining: Vec<Option<f32>> = ctx
            .visible_enemies_within(entity, my_pos, shared.danger_radius)
            .iter()
            .map(|t| ctx.attacker_escape_window(t.entity))
            .collect();
        escape_window(
            &cc_remaining,
            ctx.movement_slow_multiplier(entity),
            shared.escape_min_window,
        )
    } else {
        None
    };

    // --- DIP abort check (only while mid-dip and not being preempted) ---
    let dip_aborts = prev == Posture::Dip
        && !focused
        && dip_should_abort(state, combatant, ctx, shared, now);

    // --- DIP entry (FREE only, no pressure) ---
    let dip_entry = if prev == Posture::Free && !trigger {
        evaluate_dip_entry(entity, combatant, my_pos, auras, ctx, movement, abilities)
    } else {
        None
    };

    let next = match prev {
        // ESCAPE is committed for the whole window.
        Posture::Escape if now < state.escape_until => Posture::Escape,
        Posture::Escape if trigger => Posture::Pressured,
        Posture::Escape => Posture::Free,
        // PRESSURED hysteresis, then escape-window upgrade (same as Priest).
        Posture::Pressured if !trigger && now >= state.hold_until => Posture::Free,
        Posture::Pressured if escape_window_secs.is_some() => Posture::Escape,
        Posture::Pressured => Posture::Pressured,
        // DIP: becoming focused preempts UNCONDITIONALLY (PressuredEnter,
        // never DipAbort). Healing-heavy alone does NOT preempt a dip — the
        // teammate-HP abort (urgency threshold, below the healing-heavy
        // threshold) is the HP-based exit; after the abort lands in FREE,
        // healing-heavy flips the posture to PRESSURED on the next tick.
        Posture::Dip if focused => Posture::Pressured,
        Posture::Dip if dip_aborts => Posture::Free,
        Posture::Dip => Posture::Dip,
        // FREE: pressure first, then the dip opportunity.
        _ if trigger => Posture::Pressured,
        _ if dip_entry.is_some() => Posture::Dip,
        _ => Posture::Free,
    };

    let transitioned = next != prev;
    if transitioned {
        state.posture = next;
        state.since = now;
        state.last_direction = None;
        state.last_point = None;
        match next {
            Posture::Pressured => {
                state.hold_until = now + shared.pressured_hold;
                state.dip_target = None;
                state.dip_until = 0.0;
            }
            Posture::Escape => {
                state.escape_until = now + escape_window_secs.unwrap_or(0.0);
            }
            Posture::Dip => {
                state.dip_target = dip_entry;
                state.dip_until = now + pal.dip_budget;
                state.hold_until = 0.0;
                state.anchor = None;
            }
            _ => {
                state.hold_until = 0.0;
                state.anchor = None;
                state.dip_target = None;
                state.dip_until = 0.0;
            }
        }
    }

    let mut plan = PaladinMovementPlan::default();

    match next {
        Posture::Escape => {
            escape_tick(
                commands, entity, my_pos, ctx, state, directive, shared,
                &pal.weights, decision_trace, transitioned, prev,
            );
            plan.cast_defer = Some(shared.urgency_hp_threshold);
        }
        Posture::Pressured => paladin_pressured_tick(
            commands, entity, my_pos, ctx, state, directive, movement, now,
            decision_trace, transitioned, prev,
        ),
        Posture::Dip => {
            plan.cast_defer = Some(shared.urgency_hp_threshold);
            plan.hoj = paladin_dip_tick(
                commands, abilities, entity, my_pos, ctx, state, directive, now,
                decision_trace, transitioned, prev,
            );
        }
        _ => paladin_free_tick(commands, entity, ctx, decision_trace, transitioned, prev),
    }

    // HoJ reservation (R8) — unless the dip tick already claimed the cast.
    if !matches!(plan.hoj, HojPlan::DipCast { .. }) {
        let enemy_healer_alive = ctx
            .alive_enemies()
            .iter()
            .any(|e| e.class.is_healer());
        plan.hoj = if rotation_hoj_allowed(state.posture, enemy_healer_alive) {
            HojPlan::Rotation
        } else {
            HojPlan::Reserved
        };
    }

    if needs_insert {
        commands.entity(entity).try_insert(*state);
    }

    plan
}

/// DIP entry predicate (R8): HoJ ready (the rotation's `pre_cast_ok` gate:
/// cooldown / mana / school lockout / silence), teammate stable (most
/// injured living non-pet teammate above the urgency HP threshold and not
/// CC'd; vacuously stable with no living teammate), and an eligible enemy
/// healer within `HoJ range + dip_budget × effective speed`. Returns the
/// dip target.
fn evaluate_dip_entry(
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    movement: &MovementConfig,
    abilities: &AbilityDefinitions,
) -> Option<Entity> {
    let def = abilities.get_unchecked(&AbilityType::HammerOfJustice);

    // HoJ ready — identical readiness gate to the rotation cast.
    if !pre_cast_ok(
        AbilityType::HammerOfJustice, def, combatant, my_pos, auras, None, ctx,
        PreCastOpts::default(),
    ) {
        return None;
    }

    // Teammate stable (AE3 precondition): the would-be anchor must not need
    // us mid-walk. No living teammate (1v1 / last alive) is vacuously stable.
    let teammate = ctx
        .alive_allies()
        .into_iter()
        .filter(|a| a.entity != entity)
        .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap());
    if let Some(t) = teammate {
        if t.health_pct() <= movement.shared.urgency_hp_threshold {
            return None;
        }
        if ctx.is_ccd(t.entity) {
            return None;
        }
    }

    // Enemy healer within reach: dip_budget seconds of (slow-adjusted)
    // walking plus the cast range itself.
    let reach = def.range
        + movement.paladin.dip_budget
            * combatant.base_movement_speed
            * ctx.movement_slow_multiplier(entity);
    dip_target_candidate(ctx, combatant.team, my_pos, reach)
}

/// Mid-dip abort conditions (R8/AE3), checked each tick while DIP holds and
/// the focused preempt did not fire: budget exceeded, dip target no longer
/// HoJ-eligible (dead / immune / stun-DR-immune / stealthed), or the anchor
/// teammate's HP at/below the urgency threshold (the dip aborts WITHOUT
/// casting — the heal fires immediately after, un-deferred, because the
/// abort clears `cast_defer` before the ability pass runs this same tick).
pub fn dip_should_abort(
    state: &HealerPosture,
    combatant: &Combatant,
    ctx: &CombatContext,
    shared: &SharedMovementConfig,
    now: f32,
) -> bool {
    let Some(target) = state.dip_target else {
        return true; // defensive — DIP always carries a target
    };
    if now >= state.dip_until {
        return true; // budget exceeded
    }
    if !hoj_target_eligible(ctx, combatant.team, target) {
        return true; // target dead / immune / DR-immune / stealthed
    }
    // Teammate HP dive (AE3).
    ctx.alive_allies()
        .into_iter()
        .filter(|a| a.entity != ctx.self_entity)
        .any(|a| a.health_pct() <= shared.urgency_hp_threshold)
}

/// PRESSURED tick (R8): retreat from threats toward `fallback_range`,
/// band-holding once every threat is at/beyond the band (stand and heal /
/// self-peel), constrained to heal range of the sticky anchor ally. Reuses
/// the Priest's commitment-window + scored-direction machinery with the
/// Paladin's weights (no formation pull, no wand pull).
#[allow(clippy::too_many_arguments)]
fn paladin_pressured_tick(
    commands: &mut Commands,
    entity: Entity,
    my_pos: Vec3,
    ctx: &CombatContext,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    movement: &MovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
    transitioned: bool,
    prev: Posture,
) {
    let shared = &movement.shared;
    let pal = &movement.paladin;

    let anchor_info = select_sticky_anchor(entity, ctx, state, shared);

    // Hard commitment window (R11): re-evaluation only once the committed
    // window lapses (or the directive died), same as the Priest.
    let window_open =
        directive.map_or(false, |d| now < d.committed_until && now < d.expires);
    if window_open && !transitioned {
        return;
    }

    // Threat set: visible enemies targeting me + any visible enemy inside
    // the retreat band (fallback_range — wider than the Priest's
    // danger_radius so the retreat keeps scoring until the band is reached).
    let mut threat_positions: std::collections::BTreeMap<Entity, Vec3> = Default::default();
    for t in ctx.enemies_targeting(entity) {
        threat_positions.insert(t.entity, t.position);
    }
    for t in ctx.visible_enemies_within(entity, my_pos, pal.fallback_range) {
        threat_positions.insert(t.entity, t.position);
    }

    // Band-hold: once every threat is at/beyond fallback_range, STOP — a
    // Point directive at the current position parks the Paladin at the band
    // to heal (and self-peel: the reservation is released while PRESSURED).
    // Without the hold, the absent directive would fall through to legacy
    // melee pursuit and walk the Paladin straight back into the pressure it
    // just retreated from. Also covers healing-heavy pressure with no
    // proximate threat at all: no aimless wandering, no re-engage.
    let nearest = threat_positions
        .values()
        .map(|p| my_pos.distance(*p))
        .fold(f32::MAX, f32::min);
    if threat_positions.is_empty() || nearest >= pal.fallback_range {
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Point(my_pos),
            expires: now + shared.directive_ttl,
            committed_until: now + shared.commit_window,
        });
        state.last_direction = None;
        if transitioned {
            if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
                let trigger = if prev == Posture::Escape {
                    MovementTrigger::EscapeWindowClosed
                } else {
                    MovementTrigger::PressuredEnter
                };
                builder.transition(
                    prev.into(),
                    TracePosture::Pressured,
                    trigger,
                    // The band-hold is a Point goal (park at the band).
                    MovementGoalKind::Point,
                );
                builder.finish();
            }
        }
        return;
    }

    let inputs = ScorerInputs {
        my_pos,
        lookahead: SCORER_LOOKAHEAD,
        threats: threat_positions.into_values().collect(),
        anchor: anchor_info.map(|i| AnchorConstraint {
            pos: i.position,
            heal_range: shared.heal_range,
        }),
        formation_point: None,
        // Paladin has no wand — wand_pull is 0 in config; pass no target.
        wand_target: None,
        wand_range: shared.wand_range,
        committed_direction: state.last_direction,
    };
    let chosen = score_directions(&compass_directions_16(), &inputs, &pal.weights);
    if chosen == Vec2::ZERO {
        return; // defensive — 16 candidates always yield a direction
    }

    commands.entity(entity).try_insert(MovementDirective {
        goal: MovementGoal::Direction(chosen),
        expires: now + shared.directive_ttl,
        committed_until: now + shared.commit_window,
    });

    let direction_changed = state
        .last_direction
        .map_or(true, |d| d.distance(chosen) > 1e-3);
    state.last_direction = Some(chosen);

    if transitioned || direction_changed {
        if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
            if transitioned {
                let trigger = if prev == Posture::Escape {
                    MovementTrigger::EscapeWindowClosed
                } else {
                    // Covers FREE → PRESSURED and the DIP → PRESSURED
                    // preempt (the U3 trigger docs: preemption is
                    // PressuredEnter, not DipAbort).
                    MovementTrigger::PressuredEnter
                };
                builder.transition(
                    prev.into(),
                    TracePosture::Pressured,
                    trigger,
                    MovementGoalKind::Direction,
                );
            } else {
                builder.direction_change(
                    TracePosture::Pressured,
                    MovementTrigger::CommitExpired,
                    MovementGoalKind::Direction,
                );
            }
            builder.chosen_direction([chosen.x, chosen.y]);
            builder.finish();
        }
    }
}

/// FREE tick (R8): the Paladin's FREE is the legacy melee pursuit — NO
/// directive is ever issued (melee identity preserved). On transitions into
/// FREE the lingering directive is removed (a dip walk must stop
/// immediately) and the exit transition is traced; `DipComplete` exits are
/// emitted by the dip-cast path in `decide_paladin_action`, so a Dip → Free
/// transition seen HERE is always an abort.
fn paladin_free_tick(
    commands: &mut Commands,
    entity: Entity,
    ctx: &CombatContext,
    decision_trace: &mut DecisionTrace,
    transitioned: bool,
    prev: Posture,
) {
    if !transitioned {
        return;
    }
    // Stop any committed walk from the previous posture immediately — FREE
    // must hand movement back to legacy pursuit, and a dip directive's
    // expiry can be seconds away.
    commands.entity(entity).remove::<MovementDirective>();

    let trigger = match prev {
        Posture::Dip => MovementTrigger::DipAbort,
        Posture::Escape => MovementTrigger::EscapeWindowClosed,
        _ => MovementTrigger::PressuredExit,
    };
    if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
        // goal_kind Entity records "legacy target pursuit governs" (same
        // convention as the Priest's degenerate FREE).
        builder.transition(prev.into(), TracePosture::Free, trigger, MovementGoalKind::Entity);
        builder.finish();
    }
}

/// DIP tick (R8): keep the Entity-goal pursuit directive alive for the whole
/// budget; on arrival (within HoJ range of the dip target) hand the cast to
/// the ability pass via [`HojPlan::DipCast`]. The directive expires at the
/// budget deadline, so a stunned/feared Paladin's stale dip walk dies with
/// it (executor-side expiry).
#[allow(clippy::too_many_arguments)]
fn paladin_dip_tick(
    commands: &mut Commands,
    abilities: &AbilityDefinitions,
    entity: Entity,
    my_pos: Vec3,
    ctx: &CombatContext,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    now: f32,
    decision_trace: &mut DecisionTrace,
    transitioned: bool,
    prev: Posture,
) -> HojPlan {
    let Some(target) = state.dip_target else {
        return HojPlan::Reserved; // defensive — DIP always carries a target
    };

    let issue = |commands: &mut Commands| {
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Entity(target),
            expires: state.dip_until,
            committed_until: state.dip_until,
        });
    };

    if transitioned {
        issue(commands);
        // DipEnter carries the goal entity context via the target view.
        if let Some(mut builder) =
            start_movement_event_with_target(decision_trace, ctx, target, my_pos)
        {
            builder.transition(
                prev.into(),
                TracePosture::Dip,
                MovementTrigger::DipEnter,
                MovementGoalKind::Entity,
            );
            builder.finish();
        }
    } else if directive.is_none() {
        // Defensive re-issue (e.g., the directive died across a short CC
        // that ended before the budget) — refreshes are not decisions.
        issue(commands);
    }

    // Arrival check: within HoJ range → command the cast. The ability pass
    // re-checks readiness/eligibility/range (try_dip_hammer_of_justice) and
    // on success installs `completed_state` (DipComplete → FREE).
    let def = abilities.get_unchecked(&AbilityType::HammerOfJustice);
    let in_range = ctx
        .combatants
        .get(&target)
        .map_or(false, |t| my_pos.distance(t.position) <= def.range);
    if in_range {
        let mut completed = *state;
        completed.posture = Posture::Free;
        completed.since = now;
        completed.hold_until = 0.0;
        completed.anchor = None;
        completed.dip_target = None;
        completed.dip_until = 0.0;
        completed.last_direction = None;
        completed.last_point = None;
        HojPlan::DipCast {
            target,
            completed_state: completed,
        }
    } else {
        HojPlan::Reserved
    }
}
