//! Warlock AI Module
//!
//! Handles AI decision-making for the Warlock class.
//!
//! ## Priority Order
//! 1. Corruption (instant Shadow DoT)
//! 2. Spread curses to enemies (per-target preferences)
//! 3. Immolate (2s cast Fire DoT) - skipped when being kited
//! 4. Fear (CC on non-CC'd target)
//! 5. Drain Life (when HP < 80% and target has DoTs)
//! 6. Shadow Bolt (main damage spell) - skipped when being kited
//!
//! ## Kiting Detection
//! When being kited (slowed and out of range), the Warlock prioritizes instant-cast
//! abilities over cast-time spells that would be interrupted by movement.
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::match_config::{CharacterClass, WarlockCurse};
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::{
    ActiveAuras, AuraPending, AuraType, CastingState, ChannelingState, Combatant,
    DRCategory,
};
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::constants::GCD;
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, RejectionReason,
};

use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};

use crate::states::play_match::utils::log_ability_use;

use super::CombatContext;

/// Check if the Warlock is being kited (slowed and out of preferred range).
fn is_being_kited(
    combatant: &Combatant,
    my_pos: Vec3,
    target_pos: Vec3,
    auras: Option<&ActiveAuras>,
) -> bool {
    let is_slowed = auras
        .map(|a| a.auras.iter().any(|aura| aura.effect_type == AuraType::MovementSpeedSlow))
        .unwrap_or(false);

    let distance_to_target = my_pos.distance(target_pos);
    let preferred_range = combatant.class.preferred_range();
    let out_of_range = distance_to_target > preferred_range;

    is_slowed && out_of_range
}

/// Warlock AI: Decides and executes abilities for a Warlock combatant.
pub fn decide_warlock_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    decision_trace: &mut DecisionTrace,
) -> bool {
    // No target — no decision (emission gate).
    let Some(target_entity) = combatant.target else {
        return false;
    };

    let Some(target_info) = ctx.combatants.get(&target_entity) else {
        return false;
    };
    let target_pos = target_info.position;

    // GCD short-circuit — no event.
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let target_immune = ctx.entity_is_immune(target_entity);
    let being_kited = is_being_kited(combatant, my_pos, target_pos, auras);

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, Some(target_entity), my_pos) else {
        return false;
    };

    let enemy_has_dispeller = ctx.alive_enemies().iter().any(|e| matches!(
        e.class,
        CharacterClass::Priest | CharacterClass::Paladin
    ));

    let mut ua_attempted = false;

    // Dispeller-priority: try UA before Corruption when enemy can dispel.
    if enemy_has_dispeller && !target_immune {
        if try_unstable_affliction(
            commands, combat_log, abilities, entity, combatant, my_pos, auras,
            target_entity, target_pos, ctx, &mut builder,
        ) {
            builder.finish();
            return true;
        }
        ua_attempted = true;
    }

    // Priority 1: Corruption (instant Shadow DoT) — skip if target immune.
    if target_immune {
        builder.reject(AbilityType::Corruption, RejectionReason::TargetImmune);
    } else if try_corruption(
        commands, combat_log, abilities, entity, combatant, my_pos, auras,
        target_entity, target_pos, ctx, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 1.5: Unstable Affliction (only if not already attempted in the
    // dispeller-priority gate above).
    if target_immune {
        if !ua_attempted {
            builder.reject(AbilityType::UnstableAffliction, RejectionReason::TargetImmune);
        }
    } else if !ua_attempted {
        if try_unstable_affliction(
            commands, combat_log, abilities, entity, combatant, my_pos, auras,
            target_entity, target_pos, ctx, &mut builder,
        ) {
            builder.finish();
            return true;
        }
    }

    // Priority 2: Spread curses to all enemies.
    if try_spread_curses(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 3: Immolate.
    if target_immune {
        builder.reject(AbilityType::Immolate, RejectionReason::TargetImmune);
    } else if being_kited {
        builder.reject(
            AbilityType::Immolate,
            RejectionReason::PreconditionUnmet {
                note: "being kited — cast would be interrupted".into(),
            },
        );
    } else if try_immolate(
        commands, combat_log, abilities, entity, combatant, my_pos, auras,
        target_entity, target_pos, ctx, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 4: Fear (cc_target or kill target).
    let fear_target = combatant.cc_target.or(combatant.target);
    if let Some(fear_target_entity) = fear_target {
        if ctx.entity_is_immune(fear_target_entity) {
            builder.reject(AbilityType::Fear, RejectionReason::TargetImmune);
        } else if ctx.is_dr_immune(fear_target_entity, DRCategory::Fears) {
            builder.reject(
                AbilityType::Fear,
                RejectionReason::DRImmune { category: DRCategory::Fears },
            );
        } else if let Some(fear_target_info) = ctx.combatants.get(&fear_target_entity) {
            let fear_target_pos = fear_target_info.position;
            if try_fear(
                commands, combat_log, abilities, entity, combatant, my_pos, auras,
                fear_target_entity, fear_target_pos, ctx, &mut builder,
            ) {
                builder.finish();
                return true;
            }
        }
    } else {
        builder.reject(AbilityType::Fear, RejectionReason::NoValidTarget);
    }

    // Priority 5: Drain Life.
    if target_immune {
        builder.reject(AbilityType::DrainLife, RejectionReason::TargetImmune);
    } else if being_kited {
        builder.reject(
            AbilityType::DrainLife,
            RejectionReason::PreconditionUnmet {
                note: "being kited — channel would be interrupted".into(),
            },
        );
    } else if try_drain_life(
        commands, combat_log, abilities, entity, combatant, my_pos, auras,
        target_entity, target_pos, ctx, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 6: Shadow Bolt — skipped when being kited or target immune.
    if target_immune {
        builder.reject(AbilityType::Shadowbolt, RejectionReason::TargetImmune);
        builder.finish();
        return false;
    }
    if being_kited {
        builder.reject(
            AbilityType::Shadowbolt,
            RejectionReason::PreconditionUnmet {
                note: "being kited — cast would be interrupted".into(),
            },
        );
        builder.finish();
        return false;
    }

    let acted = try_shadowbolt(
        commands, combat_log, abilities, entity, combatant, my_pos, auras,
        target_entity, target_pos, ctx, &mut builder,
    );
    builder.finish();
    acted
}

/// Try to apply Corruption DoT to target.
fn try_corruption(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let corruption = AbilityType::Corruption;
    let corruption_def = abilities.get_unchecked(&corruption);

    let target_has_corruption = ctx.active_auras
        .get(&target_entity)
        .map(|auras| auras.iter().any(|a|
            a.effect_type == AuraType::DamageOverTime && a.ability_name == "Corruption"
        ))
        .unwrap_or(false);

    if target_has_corruption {
        builder.reject(corruption, RejectionReason::AlreadyApplied);
        return false;
    }

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        corruption, corruption_def, combatant, my_pos, auras,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            corruption,
            classify_pre_cast_failure(
                corruption, corruption_def, combatant, my_pos, auras,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(corruption, Some(target_entity), true);

    combatant.current_mana -= corruption_def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Corruption", target_tuple, "casts");

    if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, corruption_def) {
        commands.spawn(aura_pending);
    }

    combat_log.log(
        CombatLogEventType::Buff,
        format!(
            "Team {} {} applies Corruption to enemy (10 damage per 3s for 18s)",
            combatant.team,
            combatant.class.name()
        ),
    );

    info!(
        "Team {} {} applies Corruption to enemy (10 damage per 3s for 18s)",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Unstable Affliction on target.
fn try_unstable_affliction(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ua = AbilityType::UnstableAffliction;
    let ua_def = abilities.get_unchecked(&ua);

    let target_has_ua = ctx.active_auras
        .get(&target_entity)
        .map(|auras| auras.iter().any(|a|
            a.effect_type == AuraType::DamageOverTime && a.ability_name == "Unstable Affliction"
        ))
        .unwrap_or(false);

    if target_has_ua {
        builder.reject(ua, RejectionReason::AlreadyApplied);
        return false;
    }

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        ua, ua_def, combatant, my_pos, auras,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ua,
            classify_pre_cast_failure(
                ua, ua_def, combatant, my_pos, auras,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ua, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(ua_def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(ua, target_entity, cast_time));

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Unstable Affliction", target_tuple, "begins casting");

    info!(
        "Team {} {} begins casting Unstable Affliction on enemy",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Immolate on target.
fn try_immolate(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let immolate = AbilityType::Immolate;
    let immolate_def = abilities.get_unchecked(&immolate);

    let target_has_immolate = ctx.active_auras
        .get(&target_entity)
        .map(|auras| auras.iter().any(|a|
            a.effect_type == AuraType::DamageOverTime && a.ability_name == "Immolate"
        ))
        .unwrap_or(false);

    if target_has_immolate {
        builder.reject(immolate, RejectionReason::AlreadyApplied);
        return false;
    }

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        immolate, immolate_def, combatant, my_pos, auras,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            immolate,
            classify_pre_cast_failure(
                immolate, immolate_def, combatant, my_pos, auras,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(immolate, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(immolate_def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(immolate, target_entity, cast_time));

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Immolate", target_tuple, "begins casting");

    info!(
        "Team {} {} starts casting Immolate on enemy",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Fear on target.
fn try_fear(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let fear = AbilityType::Fear;
    let fear_def = abilities.get_unchecked(&fear);

    let already_ccd_type = ctx.active_auras
        .get(&target_entity)
        .and_then(|auras| {
            auras.iter().find_map(|a| {
                if matches!(a.effect_type, AuraType::Stun | AuraType::Fear | AuraType::Root) {
                    Some(a.effect_type)
                } else {
                    None
                }
            })
        });

    if let Some(cc_type) = already_ccd_type {
        builder.reject(fear, RejectionReason::TargetAlreadyCCd { cc_type });
        return false;
    }

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        fear, fear_def, combatant, my_pos, auras,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            fear,
            classify_pre_cast_failure(
                fear, fear_def, combatant, my_pos, auras,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(fear, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(fear_def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(fear, target_entity, cast_time));

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Fear", target_tuple, "begins casting");

    info!(
        "Team {} {} starts casting Fear on enemy",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Shadow Bolt on target.
fn try_shadowbolt(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let shadowbolt = AbilityType::Shadowbolt;
    let shadowbolt_def = abilities.get_unchecked(&shadowbolt);

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        shadowbolt, shadowbolt_def, combatant, my_pos, auras,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            shadowbolt,
            classify_pre_cast_failure(
                shadowbolt, shadowbolt_def, combatant, my_pos, auras,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(shadowbolt, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(shadowbolt_def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(shadowbolt, target_entity, cast_time));

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Shadow Bolt", target_tuple, "begins casting");

    info!(
        "Team {} {} starts casting {} on enemy",
        combatant.team,
        combatant.class.name(),
        shadowbolt_def.name
    );

    true
}

/// Try to channel Drain Life on target.
fn try_drain_life(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let drain_life = AbilityType::DrainLife;
    let drain_life_def = abilities.get_unchecked(&drain_life);

    let hp_percent = combatant.current_health / combatant.max_health;
    if hp_percent >= 0.8 {
        builder.reject(
            drain_life,
            RejectionReason::PreconditionUnmet {
                note: "HP above 80% — Drain Life is healing-gated".into(),
            },
        );
        return false;
    }

    let target_has_dot = ctx.active_auras
        .get(&target_entity)
        .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::DamageOverTime))
        .unwrap_or(false);

    if !target_has_dot {
        builder.reject(
            drain_life,
            RejectionReason::PreconditionUnmet {
                note: "no DoT on target — Drain Life maintains pressure".into(),
            },
        );
        return false;
    }

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        drain_life, drain_life_def, combatant, my_pos, auras,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            drain_life,
            classify_pre_cast_failure(
                drain_life, drain_life_def, combatant, my_pos, auras,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(drain_life, Some(target_entity), false);

    combatant.current_mana -= drain_life_def.mana_cost;
    combatant.global_cooldown = GCD;

    let channel_duration = drain_life_def.channel_duration.unwrap_or(5.0);
    let tick_interval = drain_life_def.channel_tick_interval;

    commands.entity(entity).insert(ChannelingState {
        ability: drain_life,
        duration_remaining: channel_duration,
        time_until_next_tick: tick_interval,
        tick_interval,
        target: target_entity,
        interrupted: false,
        interrupted_display_time: 0.0,
        ticks_applied: 0,
    });

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Drain Life", target_tuple, "begins channeling");

    info!(
        "Team {} {} starts channeling Drain Life on enemy (HP: {:.0}%)",
        combatant.team,
        combatant.class.name(),
        hp_percent * 100.0
    );

    true
}

/// Try to spread curses to all enemies based on per-target preferences.
fn try_spread_curses(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    if combatant.warlock_curse_prefs.is_empty() {
        // No curse preferences configured — silently skip (don't litter the trace
        // with NoValidTarget for a feature this Warlock isn't using).
        return false;
    }

    let mut enemies: Vec<(Entity, Vec3, u8)> = ctx.combatants
        .iter()
        .filter_map(|(&enemy_entity, info)| {
            if info.team != combatant.team && info.current_health > 0.0 && !info.is_pet {
                Some((enemy_entity, info.position, info.slot))
            } else {
                None
            }
        })
        .collect();

    enemies.sort_by_key(|(_, _, slot)| *slot);

    for (enemy_entity, enemy_pos, enemy_slot) in enemies {
        if ctx.entity_is_immune(enemy_entity) {
            continue;
        }

        let curse_pref = combatant
            .warlock_curse_prefs
            .get(enemy_slot as usize)
            .copied()
            .unwrap_or(WarlockCurse::Agony);

        let has_our_curse = ctx.active_auras
            .get(&enemy_entity)
            .map(|auras| {
                auras.iter().any(|a| {
                    a.caster == Some(entity)
                        && (a.ability_name == "Curse of Agony"
                            || a.ability_name == "Curse of Weakness"
                            || a.ability_name == "Curse of Tongues")
                })
            })
            .unwrap_or(false);

        if has_our_curse {
            continue;
        }

        let (ability, ability_name) = match curse_pref {
            WarlockCurse::Agony => (AbilityType::CurseOfAgony, "Curse of Agony"),
            WarlockCurse::Weakness => (AbilityType::CurseOfWeakness, "Curse of Weakness"),
            WarlockCurse::Tongues => (AbilityType::CurseOfTongues, "Curse of Tongues"),
        };

        if try_cast_curse(
            commands, combat_log, abilities, entity, combatant, my_pos, auras,
            enemy_entity, enemy_pos, ctx, ability, ability_name, builder,
        ) {
            return true;
        }
    }

    false
}

/// Cast a specific curse on a target.
fn try_cast_curse(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    target_entity: Entity,
    target_pos: Vec3,
    ctx: &CombatContext,
    ability: AbilityType,
    ability_name: &str,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability_def = abilities.get_unchecked(&ability);

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        ability, ability_def, combatant, my_pos, auras,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, ability_def, combatant, my_pos, auras,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), true);

    combatant.current_mana -= ability_def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, ability_name, target_tuple, "casts");

    if let Some(aura_pending) = AuraPending::from_ability(target_entity, entity, ability_def) {
        commands.spawn(aura_pending);
    }

    let effect_description = match ability {
        AbilityType::CurseOfAgony => "14 damage per 4s for 24s",
        AbilityType::CurseOfWeakness => "-20% physical damage for 2 min",
        AbilityType::CurseOfTongues => "+50% cast time for 30s",
        _ => "",
    };

    combat_log.log(
        CombatLogEventType::Buff,
        format!(
            "Team {} {} applies {} to enemy ({})",
            combatant.team,
            combatant.class.name(),
            ability_name,
            effect_description
        ),
    );

    info!(
        "Team {} {} applies {} to enemy ({})",
        combatant.team,
        combatant.class.name(),
        ability_name,
        effect_description
    );

    true
}
