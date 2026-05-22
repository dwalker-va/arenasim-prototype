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
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{
    CRITICAL_HP_THRESHOLD, DIVINE_SHIELD_HP_THRESHOLD, GCD, HEALTHY_HP_THRESHOLD,
    HOLY_SHOCK_DAMAGE_RANGE, LOW_HP_THRESHOLD, SAFE_HEAL_MAX_THRESHOLD,
};
use crate::states::play_match::decision_trace::{
    ActorView, DecisionEventBuilder, DecisionTrace, RejectionReason, TargetView,
};
use crate::states::play_match::utils::{combatant_id, log_ability_use};

use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};

use super::{CombatContext, CombatantInfo};

/// Paladin AI: Decides and executes abilities for a Paladin combatant.
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
    decision_trace: &mut DecisionTrace,
) -> bool {
    // GCD short-circuit — no event.
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let actor_view = match ctx.self_info() {
        Some(info) => ActorView::from_info(info),
        None => return false,
    };
    let target_view = combatant
        .target
        .and_then(|t| ctx.combatants.get(&t))
        .map(|info| TargetView::from_info(info, my_pos));

    let mut builder = decision_trace.start_ability_decision(actor_view, target_view);

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

    // Priority 4: Hammer of Justice.
    if try_hammer_of_justice(
        commands, combat_log, abilities, combatant, my_pos, auras, ctx,
        same_frame_cc_queue, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 5: Flash of Light.
    if try_flash_of_light(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 6: Holy Light.
    if try_holy_light(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        &mut builder,
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
fn try_flash_of_light(
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
fn try_holy_light(
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

/// Try Hammer of Justice on an enemy in melee range.
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
        .filter(|(_, info)| {
            info.team != combatant.team && info.current_health > 0.0 && !info.stealthed && !info.is_pet
        })
        .filter(|(e, _)| !ctx.entity_is_immune(**e) && !ctx.is_dr_immune(**e, DRCategory::Stuns))
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

    builder.choose(ability, Some(*target_entity), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;
    combatant.ability_cooldowns.insert(ability, def.cooldown);

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
        same_frame_cc_queue.push((*target_entity, hoj_aura.clone()));
        commands.spawn(AuraPending {
            target: *target_entity,
            aura: hoj_aura,
        });
    }

    true
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
