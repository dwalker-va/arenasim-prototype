//! Priest AI Module
//!
//! Handles AI decision-making for the Priest class.
//!
//! ## Priority Order
//! 1. Power Word: Fortitude (buff all allies pre-combat)
//! 2. Dispel Magic - Urgent (Polymorph, Fear - complete loss of control)
//! 3. Power Word: Shield (shield low-health allies)
//! 4. Flash Heal (heal injured allies)
//! 5. Dispel Magic - Maintenance (Roots, DoTs when team HP is stable)
//! 6. Mind Blast (damage when allies are healthy)
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;
use std::collections::HashSet;

use crate::combat::log::CombatLog;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::{AbilityConfig, AbilityDefinitions};
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::{calculate_cast_time, clamp_to_arena};
use crate::states::play_match::constants::GCD;
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, MovementGoalKind, MovementTrigger,
    Posture as TracePosture, RejectionReason,
};
use crate::states::play_match::movement_config::{MovementConfig, SharedMovementConfig};
use crate::states::play_match::utils::{combatant_id, log_ability_use, spawn_speech_bubble};

use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};
use super::healer_postures::{
    compound_pressure_trigger, escape_tick, escape_window_from, healer_pressured_tick_shared,
    start_movement_event, start_movement_event_with_target,
};

use super::CombatContext;

/// Per-tick output of [`evaluate_priest_posture`], threaded into
/// [`decide_priest_action`] (mirrors the Paladin's `PaladinMovementPlan`):
/// the escape-defer urgency input plus the Psychic Scream dip gate.
pub struct PriestMovementPlan {
    /// `Some(urgency_hp_threshold)` while an ESCAPE window OR a DIP is live:
    /// the heal ladder defers non-critical movement-locking casts (R7).
    pub escape_defer: Option<f32>,
    /// Psychic Scream gate for this tick (reservation / dip cast). Always
    /// `Rotation` until U4 wires the offensive dip.
    pub scream_dip: ScreamDipPlan,
}

impl Default for PriestMovementPlan {
    fn default() -> Self {
        Self {
            escape_defer: None,
            scream_dip: ScreamDipPlan::Rotation,
        }
    }
}

/// How the rotation may use Psychic Scream this tick (mirrors `HojPlan`).
/// `Reserved` and `DipCast` are constructed in U4 (the offensive dip); U3
/// scaffolds the type and always returns `Rotation`.
#[allow(dead_code)]
pub enum ScreamDipPlan {
    /// No reservation: the defensive scream behaves exactly as in U2.
    Rotation,
    /// A living enemy healer exists and the Priest is not pressured: the
    /// defensive scream is suppressed — the cooldown is saved for the dip.
    Reserved,
    /// Mid-dip and within scream radius of the dip target: cast Psychic
    /// Scream now. On a successful cast the caller installs `completed_state`
    /// (posture back to FREE — DipComplete) and removes the walk directive.
    DipCast {
        target: Entity,
        completed_state: HealerPosture,
    },
}

/// Priest AI: Decides and executes abilities for a Priest combatant.
///
/// `escape_defer` is the cast-vs-move urgency input (R7/AE1), returned by
/// `evaluate_priest_posture`: `Some(urgency_hp_threshold)` while an ESCAPE
/// window is live. While set, NON-critical movement-locking casts are
/// deferred — Flash Heal is skipped when the would-be heal target's HP
/// fraction is above the threshold (movement wins), and Mind Blast (a damage
/// cast, never critical) is skipped outright. At or below the threshold the
/// heal fires normally: critical heals always win. Instants (PW: Shield,
/// PW: Fortitude, Dispel Magic) are NOT deferred — they trigger the GCD but
/// never insert `CastingState`, so they don't lock movement (the directive
/// keeps executing through the GCD). R12 holds structurally: an in-progress
/// cast is never reached by this function, let alone canceled.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
pub fn decide_priest_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    shielded_this_frame: &mut HashSet<Entity>,
    fortified_this_frame: &mut HashSet<Entity>,
    plan: &PriestMovementPlan,
    movement: &MovementConfig,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    decision_trace: &mut DecisionTrace,
) -> bool {
    let escape_defer = plan.escape_defer;
    // GCD short-circuit — no event (emission gate).
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, combatant.target, my_pos) else {
        return false;
    };

    // Priority 1: Power Word: Fortitude (buff allies)
    if try_fortitude(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        fortified_this_frame, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 2: Dispel Magic - Urgent (Polymorph, Fear)
    if try_dispel_magic(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        90, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 2.5: DIP Psychic Scream (U4 offensive dip). The dip walked up to
    // dip_budget seconds for exactly this AoE fear on the enemy healer — it
    // outranks everything below the urgent dispel. On success the posture
    // returns to FREE (DipComplete) and the walk directive dies with it; the
    // return to backline happens naturally via FREE formation.
    if let ScreamDipPlan::DipCast { target, completed_state } = &plan.scream_dip {
        if try_dip_psychic_scream(
            commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
            same_frame_cc_queue, &mut builder,
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

    // Priority 3: Psychic Scream (defensive AoE fear peel / escape opener).
    // The defensive use only fires under pressure; a dip runs only when NOT
    // pressured, so the two are mutually exclusive — the pressured gate is the
    // scream's reservation (no explicit Reserved state needed, R14).
    if try_psychic_scream(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        &movement.shared, same_frame_cc_queue, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 4: Power Word: Shield
    if try_power_word_shield(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        shielded_this_frame, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 4: Flash Heal
    if try_flash_heal(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        escape_defer, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Priority 5: Dispel Magic - Maintenance (only when team healthy)
    if ctx.is_team_healthy(0.70, my_pos) {
        if try_dispel_magic(
            commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
            50, &mut builder,
        ) {
            builder.finish();
            return true;
        }
    }

    // Priority 6: Mind Blast
    if try_mind_blast(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        escape_defer, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    builder.finish();
    false
}

/// Try to cast Psychic Scream — instant self-centered AoE Fear (R1/R2/R5/R9/R10).
///
/// Defensive self-peel / escape opener: fires only when the Priest is genuinely
/// pressured (`compound_pressure_trigger`) AND at least one fear-eligible enemy
/// is inside the scream's radius. Because the radius (~8yd) is point-blank for a
/// cloth healer, this single gate covers both the surrounded case and the
/// chased-into-melee escape-opener case — the fear sends chasers running away,
/// compounding the escape. Mirrors the Frost Nova self-AoE shape
/// (`mage.rs::try_frost_nova`) minus the damage half. The not-pressured
/// (offensive dip) use is owned by the posture machine (U4), not this function.
///
/// AoE target filtering is the caller's responsibility: `pre_cast_ok` guards a
/// single target, so per-target immunity / Fear-DR filtering happens here.
fn try_psychic_scream(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    shared: &SharedMovementConfig,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let scream = AbilityType::PsychicScream;
    let scream_def = abilities.get_unchecked(&scream);

    let opts = PreCastOpts::default();
    if !pre_cast_ok(scream, scream_def, combatant, my_pos, auras, None, ctx, opts) {
        builder.reject(
            scream,
            classify_pre_cast_failure(scream, scream_def, combatant, my_pos, auras, None, ctx, opts),
        );
        return false;
    }

    // Defensive gate (R9/R10): only fire under genuine pressure, so the panic
    // button is never burned on incidental proximity.
    if !compound_pressure_trigger(entity, my_pos, ctx, shared) {
        builder.reject(
            scream,
            RejectionReason::PreconditionUnmet { note: "not pressured".into() },
        );
        return false;
    }

    // Defer to a critical ally heal: a dying heal target (at/below the urgency
    // HP threshold, within heal range) must be healed before the peel. The
    // scream is priority 3 — above Flash Heal — so without this gate it would
    // burn a GCD and delay a life-saving heal, breaking the critical-heal-wins
    // invariant (see `try_flash_heal`). The scream still fires next GCD.
    if ctx
        .lowest_health_ally_below(shared.urgency_hp_threshold, shared.heal_range, my_pos)
        .is_some()
    {
        builder.reject(
            scream,
            RejectionReason::PreconditionUnmet { note: "critical heal pending".into() },
        );
        return false;
    }

    let targets = scream_targets(ctx, entity, my_pos, scream_def.range);
    if targets.is_empty() {
        builder.reject(scream, RejectionReason::NoValidTarget);
        return false;
    }

    fire_psychic_scream(
        commands, combat_log, scream_def, entity, combatant, same_frame_cc_queue, &targets, builder,
    );
    true
}

/// Fear-eligible enemies within `radius` of `my_pos`: visible + alive (helper),
/// not immune, not Fear-DR-immune. Shared by the defensive predicate and the
/// offensive dip cast. AoE filtering is the caller's job (R5).
fn scream_targets(
    ctx: &CombatContext,
    entity: Entity,
    my_pos: Vec3,
    radius: f32,
) -> Vec<(Entity, u8, CharacterClass)> {
    ctx.visible_enemies_within(entity, my_pos, radius)
        .into_iter()
        .filter(|info| {
            !ctx.entity_is_immune(info.entity) && !ctx.is_dr_immune(info.entity, DRCategory::Fears)
        })
        .map(|info| (info.entity, info.team, info.class))
        .collect()
}

/// Apply Psychic Scream: record the choice, spend GCD/mana/cooldown, queue Fear
/// on each target (same-frame CC visible), and log. Assumes `targets` is
/// non-empty and readiness is already checked by the caller.
#[allow(clippy::too_many_arguments)]
fn fire_psychic_scream(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    scream_def: &AbilityConfig,
    entity: Entity,
    combatant: &mut Combatant,
    same_frame_cc_queue: &mut Vec<(Entity, Aura)>,
    targets: &[(Entity, u8, CharacterClass)],
    builder: &mut DecisionEventBuilder<'_>,
) {
    builder.choose(AbilityType::PsychicScream, None, true);

    spawn_speech_bubble(commands, entity, "Psychic Scream");
    combatant.current_mana -= scream_def.mana_cost;
    combatant
        .ability_cooldowns
        .insert(AbilityType::PsychicScream, scream_def.cooldown);
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, "Psychic Scream", None, "casts");

    let fear_duration = scream_def.applies_aura.as_ref().map(|a| a.duration).unwrap_or(0.0);
    for (target_entity, target_team, target_class) in targets {
        if let Some(aura_pending) = AuraPending::from_ability(*target_entity, entity, scream_def) {
            same_frame_cc_queue.push((*target_entity, aura_pending.aura.clone()));
            commands.spawn(aura_pending);
        }

        let message = format!(
            "Team {} {}'s Psychic Scream fears Team {} {} ({:.1}s)",
            combatant.team,
            combatant.class.name(),
            target_team,
            target_class.name(),
            fear_duration
        );
        combat_log.log_crowd_control(
            combatant_id(combatant.team, combatant.class),
            combatant_id(*target_team, *target_class),
            "Fear".to_string(),
            fear_duration,
            message,
        );
    }

    info!(
        "Team {} {} casts Psychic Scream! (AOE fear) - {} enemies feared",
        combatant.team,
        combatant.class.name(),
        targets.len()
    );
}

/// Offensive dip cast (U4): on dip arrival, fire Psychic Scream as a
/// self-centered AoE. The dip target only drove the walk — the cast fears every
/// fear-eligible enemy in radius (the enemy healer plus anyone else caught).
/// Readiness is re-checked (mana/cooldown can shift across the walk); the
/// pressured gate is intentionally absent (a dip runs only when NOT pressured).
/// Returns true iff the scream fired (≥1 enemy in radius).
fn try_dip_psychic_scream(
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
    let scream = AbilityType::PsychicScream;
    let scream_def = abilities.get_unchecked(&scream);

    let opts = PreCastOpts::default();
    if !pre_cast_ok(scream, scream_def, combatant, my_pos, auras, None, ctx, opts) {
        builder.reject(
            scream,
            classify_pre_cast_failure(scream, scream_def, combatant, my_pos, auras, None, ctx, opts),
        );
        return false;
    }

    let targets = scream_targets(ctx, entity, my_pos, scream_def.range);
    if targets.is_empty() {
        builder.reject(scream, RejectionReason::NoValidTarget);
        return false;
    }

    fire_psychic_scream(
        commands, combat_log, scream_def, entity, combatant, same_frame_cc_queue, &targets, builder,
    );
    true
}

/// Try to cast Power Word: Fortitude on an unbuffed ally.
fn try_fortitude(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    fortified_this_frame: &mut HashSet<Entity>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::PowerWordFortitude;
    let def = abilities.get_unchecked(&ability);

    let mut unbuffed_ally: Option<(Entity, Vec3)> = None;

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != combatant.team || info.current_health <= 0.0 || info.is_pet {
            continue;
        }
        let has_fortitude = ctx.active_auras
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.effect_type == AuraType::MaxHealthIncrease))
            .unwrap_or(false);
        if has_fortitude {
            continue;
        }
        if fortified_this_frame.contains(ally_entity) {
            continue;
        }
        unbuffed_ally = Some((*ally_entity, info.position));
        break;
    }

    let Some((buff_target, target_pos)) = unbuffed_ally else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        ability, def, combatant, my_pos, auras,
        Some((buff_target, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, auras,
                Some((buff_target, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(buff_target), true);

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&buff_target).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Power Word: Fortitude", target_tuple, "casts");

    if let Some(aura_pending) = AuraPending::from_ability(buff_target, entity, def) {
        commands.spawn(aura_pending);
    }

    fortified_this_frame.insert(buff_target);

    info!(
        "Team {} {} casts Power Word: Fortitude on ally",
        combatant.team,
        combatant.class.name()
    );

    true
}

/// Try to cast Power Word: Shield on an ally.
fn try_power_word_shield(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    shielded_this_frame: &mut HashSet<Entity>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let pw_shield = AbilityType::PowerWordShield;
    let pw_shield_def = abilities.get_unchecked(&pw_shield);

    if combatant.current_mana < pw_shield_def.mana_cost {
        builder.reject(
            pw_shield,
            RejectionReason::InsufficientMana {
                have: combatant.current_mana,
                need: pw_shield_def.mana_cost,
            },
        );
        return false;
    }

    let mut best_candidate: Option<(Entity, Vec3, f32)> = None;

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != combatant.team || info.current_health <= 0.0 || info.is_pet {
            continue;
        }
        let ally_auras = ctx.active_auras.get(ally_entity);
        let has_weakened_soul = ally_auras
            .map_or(false, |auras| auras.iter().any(|a| a.effect_type == AuraType::WeakenedSoul));
        let has_pw_shield = ally_auras.map_or(false, |auras| {
            auras
                .iter()
                .any(|a| a.effect_type == AuraType::Absorb && a.ability_name == "Power Word: Shield")
        });
        let shielded_this_frame_check = shielded_this_frame.contains(ally_entity);

        if has_weakened_soul || has_pw_shield || shielded_this_frame_check {
            continue;
        }

        let hp_percent = info.current_health / info.max_health;
        let is_full_hp = hp_percent >= 1.0;
        let is_below_threshold = hp_percent < 0.7;

        if is_full_hp || is_below_threshold {
            match best_candidate {
                None => best_candidate = Some((*ally_entity, info.position, hp_percent)),
                Some((_, _, best_percent)) if hp_percent < best_percent => {
                    best_candidate = Some((*ally_entity, info.position, hp_percent));
                }
                _ => {}
            }
        }
    }

    let Some((shield_entity, target_pos, _)) = best_candidate else {
        builder.reject(pw_shield, RejectionReason::NoValidTarget);
        return false;
    };

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        pw_shield, pw_shield_def, combatant, my_pos, auras,
        Some((shield_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            pw_shield,
            classify_pre_cast_failure(
                pw_shield, pw_shield_def, combatant, my_pos, auras,
                Some((shield_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(pw_shield, Some(shield_entity), true);

    combatant.current_mana -= pw_shield_def.mana_cost;
    combatant.global_cooldown = GCD;

    let target_tuple = ctx.combatants.get(&shield_entity).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, "Power Word: Shield", target_tuple, "casts");

    if let Some(aura_pending) = AuraPending::from_ability(shield_entity, entity, pw_shield_def) {
        commands.spawn(aura_pending);
    }

    commands.spawn(AuraPending {
        target: shield_entity,
        aura: Aura {
            effect_type: AuraType::WeakenedSoul,
            duration: 15.0,
            magnitude: 0.0,
            break_on_damage_threshold: -1.0,
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: Some(entity),
            ability_name: "Weakened Soul".to_string(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None,
            applied_this_frame: false,
            backlash_damage: None,
        },
    });

    shielded_this_frame.insert(shield_entity);

    true
}

/// Try to cast Dispel Magic on an ally with a dispellable debuff.
/// Delegates to the shared `try_dispel_ally()` in `class_ai/mod.rs`, which
/// emits its own reject/choose events to the builder.
fn try_dispel_magic(
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
        AbilityType::DispelMagic,
        "[DISPEL]",
        "Dispel Magic",
        CharacterClass::Priest,
        builder,
    )
}

/// Try to cast Flash Heal on the lowest HP ally.
///
/// Cast-vs-move urgency (R7/AE1): while `escape_defer` is `Some(threshold)`
/// (a live ESCAPE window) and the would-be heal target's HP fraction is
/// ABOVE the threshold, the heal is deferred — Flash Heal locks movement for
/// its whole cast, which would freeze the Priest mid-escape and waste the
/// window. At or below the threshold the heal fires normally (critical heals
/// always win, even at the cost of the window).
fn try_flash_heal(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    escape_defer: Option<f32>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::FlashHeal;
    let def = abilities.get_unchecked(&ability);

    let Some(target_info) = ctx.lowest_health_ally_below(0.9, def.range, my_pos) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };
    let heal_target = target_info.entity;
    let target_pos = target_info.position;

    if let Some(threshold) = escape_defer {
        if target_info.health_pct() > threshold {
            builder.reject(
                ability,
                RejectionReason::PreconditionUnmet {
                    note: "escape window live: non-critical heal deferred".to_string(),
                },
            );
            return false;
        }
    }

    let opts = PreCastOpts::default();
    if !pre_cast_ok(
        ability, def, combatant, my_pos, auras,
        Some((heal_target, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, auras,
                Some((heal_target, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(heal_target), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(ability, heal_target, cast_time));

    let target_tuple = ctx.combatants
        .get(&heal_target)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, target_tuple, "begins casting");

    info!(
        "Team {} {} starts casting {} on ally",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to cast Mind Blast on the current target.
///
/// Deferred outright while an ESCAPE window is live (`escape_defer` is
/// `Some`): Mind Blast is a movement-locking cast and is never critical, so
/// letting it fire after a deferred Flash Heal would freeze the Priest
/// mid-escape anyway — defeating the deferral one priority rung above it.
fn try_mind_blast(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    escape_defer: Option<f32>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::MindBlast;
    let def = abilities.get_unchecked(&ability);

    if escape_defer.is_some() {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "escape window live: movement-locking cast deferred".to_string(),
            },
        );
        return false;
    }

    let Some(target_entity) = combatant.target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    let Some(target_info) = ctx.combatants.get(&target_entity) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    let target_pos = target_info.position;

    let opts = PreCastOpts {
        check_friendly_cc: true,
        check_target_immune: true,
        ..Default::default()
    };
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

    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);

    commands.entity(entity).insert(CastingState::new(ability, target_entity, cast_time));

    let target_tuple = ctx.combatants
        .get(&target_entity)
        .map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, target_tuple, "begins casting");

    info!(
        "Team {} {} starts casting {} on enemy",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

// ============================================================================
// Posture evaluation (healer movement AI — U6: FREE/PRESSURED, U7: ESCAPE)
// ============================================================================

/// ESCAPE window math (R7) lives in [`super::healer_postures`] (shared with
/// the Paladin). Re-exported here so the public `class_ai::priest::` path —
/// and the `escape_window_math` probe suite that imports it — keeps working.
pub use super::healer_postures::{escape_distance_gained, escape_window};

// FREE-directive tuning (formation_shift_threshold / formation_deadzone /
// directive_refresh_margin) is data-driven via `movement.priest` — see
// `PriestMovementConfig` in movement_config.rs and the priest block in
// assets/config/movement.ron (RON-first policy).

/// Per-target Psychic Scream dip eligibility (mirrors `hoj_target_eligible`,
/// keyed to Fear-DR): alive enemy non-pet, not stealthed, not immune, not
/// Fear-DR-immune.
fn scream_dip_target_eligible(ctx: &CombatContext, my_team: u8, target: Entity) -> bool {
    let Some(info) = ctx.combatants.get(&target) else {
        return false;
    };
    info.team != my_team
        && info.current_health > 0.0
        && !info.stealthed
        && !info.is_pet
        && !ctx.entity_is_immune(target)
        && !ctx.is_dr_immune(target, DRCategory::Fears)
}

/// Nearest fear-eligible enemy healer within `reach` (mirrors
/// `dip_target_candidate`). Drives only the dip walk goal; the self-centered
/// AoE cast on arrival fears every enemy in radius, not just this target.
fn scream_dip_target_candidate(
    ctx: &CombatContext,
    my_team: u8,
    my_pos: Vec3,
    reach: f32,
) -> Option<Entity> {
    ctx.alive_enemies()
        .into_iter()
        .filter(|e| e.class.is_healer())
        .filter(|e| scream_dip_target_eligible(ctx, my_team, e.entity))
        .filter(|e| my_pos.distance(e.position) <= reach)
        .min_by(|a, b| {
            my_pos
                .distance(a.position)
                .partial_cmp(&my_pos.distance(b.position))
                .unwrap()
        })
        .map(|e| e.entity)
}

/// DIP entry predicate (U4, mirrors `evaluate_dip_entry`): scream ready (the
/// rotation `pre_cast_ok` gate), no teammate in trouble (deferral — any living
/// non-pet teammate below `healing_heavy_hp` blocks the dip so the Priest heals
/// instead), the anchor teammate not CC'd, and an eligible enemy healer within
/// `scream radius + dip_budget × effective speed`. Returns the dip target.
fn evaluate_scream_dip_entry(
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    movement: &MovementConfig,
    abilities: &AbilityDefinitions,
) -> Option<Entity> {
    let def = abilities.get_unchecked(&AbilityType::PsychicScream);

    if !pre_cast_ok(
        AbilityType::PsychicScream, def, combatant, my_pos, auras, None, ctx,
        PreCastOpts::default(),
    ) {
        return None;
    }

    // Deferral (R12): aggressive by default, but if any living teammate is in
    // trouble (below healing_heavy_hp), stay back and heal rather than dip.
    let teammate = ctx
        .alive_allies()
        .into_iter()
        .filter(|a| a.entity != entity)
        .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap());
    if let Some(t) = teammate {
        if t.health_pct() < movement.priest.healing_heavy_hp {
            return None;
        }
        if ctx.is_ccd(t.entity) {
            return None;
        }
    }

    let reach = def.range
        + movement.priest.dip_budget
            * combatant.base_movement_speed
            * ctx.movement_slow_multiplier(entity);
    scream_dip_target_candidate(ctx, combatant.team, my_pos, reach)
}

/// Mid-dip abort (U4, mirrors `dip_should_abort`): budget exceeded, the dip
/// target no longer fear-eligible (dead / immune / DR-immune / stealthed), or a
/// teammate's HP at/below the urgency threshold (abort WITHOUT casting — the
/// heal fires un-deferred the same tick because the abort clears `cast_defer`).
fn scream_dip_should_abort(
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
    if !scream_dip_target_eligible(ctx, combatant.team, target) {
        return true; // target dead / immune / DR-immune / stealthed
    }
    ctx.alive_allies()
        .into_iter()
        .filter(|a| a.entity != ctx.self_entity)
        .any(|a| a.health_pct() <= shared.urgency_hp_threshold)
}

/// DIP tick (U4, mirrors `paladin_dip_tick`): keep the Entity-goal walk alive
/// for the whole budget; on arrival (dip target within scream radius of me)
/// hand the cast to the ability pass via [`ScreamDipPlan::DipCast`]. The
/// directive expires at the budget deadline, so a CC'd Priest's stale dip walk
/// dies with it (executor-side expiry).
#[allow(clippy::too_many_arguments)]
fn priest_dip_tick(
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
) -> ScreamDipPlan {
    let Some(target) = state.dip_target else {
        return ScreamDipPlan::Rotation; // defensive — DIP always carries a target
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
        // DipEnter carries the goal entity (enemy healer) via the target view.
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
        // Defensive re-issue (directive died across a short CC) — not a decision.
        issue(commands);
    }

    // Arrival: dip target within scream radius → command the AoE cast. The
    // ability pass re-checks readiness (try_dip_psychic_scream) and on success
    // installs `completed_state` (DipComplete → FREE).
    let def = abilities.get_unchecked(&AbilityType::PsychicScream);
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
        ScreamDipPlan::DipCast {
            target,
            completed_state: completed,
        }
    } else {
        ScreamDipPlan::Rotation
    }
}

/// Evaluate the Priest's movement posture (FREE/PRESSURED/ESCAPE) and
/// issue/refresh a [`MovementDirective`] accordingly. Runs at the top of the
/// Priest's decide tick, BEFORE the GCD short-circuit (the GCD locks casts,
/// not legs), and only after gates open (the caller gates on
/// `countdown.gates_opened` — no pre-match directives or trace events).
///
/// R12 is structural: casting Priests never reach this function
/// (`decide_abilities` excludes `CastingState`/`ChannelingState` entities)
/// and `move_to_target` blocks directive execution while casting — posture
/// movement happens in cast gaps and never cancels an in-progress cast.
///
/// Trace emission (R3): posture transitions and committed direction /
/// formation re-commit changes only — never per-tick.
///
/// Returns `Some(urgency_hp_threshold)` while an ESCAPE window is live — the
/// caller threads it into `decide_priest_action`, whose heal priority defers
/// non-critical movement-locking casts for the window (R7 cast-vs-move
/// urgency; AE1). `None` otherwise.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_priest_posture(
    commands: &mut Commands,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    // `abilities` / `auras` are wired for the U4 offensive dip (the dip-entry
    // predicate needs the Psychic Scream def for reach/eligibility and auras
    // for pre_cast_ok). Inert in U3 — the posture machine returns Rotation.
    abilities: &AbilityDefinitions,
    auras: Option<&ActiveAuras>,
    posture: Option<&mut HealerPosture>,
    directive: Option<&MovementDirective>,
    movement: &MovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
) -> PriestMovementPlan {
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

    // --- PRESSURED compound trigger (R6) --- see
    // `healer_postures::compound_pressure_trigger` (shared with the Paladin).
    let trigger = compound_pressure_trigger(entity, my_pos, ctx, shared);

    let prev = state.posture;

    // --- ESCAPE entry window (R7) --- evaluated only while currently
    // PRESSURED with the trigger still live: every visible threat within the
    // danger radius must be movement-impaired (Root/Stun/Incapacitate — Fear
    // excluded by `attacker_escape_window`: a feared attacker self-solves,
    // and a Fear-only window must NOT trigger ESCAPE). One unimpaired
    // proximate threat voids the window; sub-cutoff windows (slow-adjusted)
    // are ignored. See `escape_window` for the full rule set.
    let escape_window_secs = if prev == Posture::Pressured && trigger {
        escape_window_from(
            ctx.visible_enemies_within(entity, my_pos, shared.danger_radius)
                .iter()
                .map(|t| ctx.attacker_escape_window(t.entity)),
            ctx.movement_slow_multiplier(entity),
            shared.escape_min_window,
        )
    } else {
        None
    };

    // --- DIP abort check (only while mid-dip and not being preempted) ---
    let dip_aborts = prev == Posture::Dip
        && !trigger
        && scream_dip_should_abort(state, combatant, ctx, shared, now);

    // --- DIP entry (FREE only, no pressure): the offensive scream dip ---
    let dip_entry = if prev == Posture::Free && !trigger {
        evaluate_scream_dip_entry(entity, combatant, my_pos, auras, ctx, movement, abilities)
    } else {
        None
    };

    let next = match prev {
        // ESCAPE is committed for the whole window (no re-evaluation churn
        // mid-escape); the window end is an absolute sim-time deadline.
        Posture::Escape if now < state.escape_until => Posture::Escape,
        // Window expired: → PRESSURED if still threatened, else FREE.
        Posture::Escape if trigger => Posture::Pressured,
        Posture::Escape => Posture::Free,
        // Hysteresis: PRESSURED may not relax before `hold_until` even when
        // the trigger momentarily drops (threat hovering at the radius must
        // not strobe the posture). Exiting requires the trigger false.
        Posture::Pressured if !trigger && now >= state.hold_until => Posture::Free,
        Posture::Pressured if escape_window_secs.is_some() => Posture::Escape,
        Posture::Pressured => Posture::Pressured,
        // DIP (U4): becoming focused preempts UNCONDITIONALLY (→ PRESSURED,
        // never DipAbort — the defensive self-peel takes over). Otherwise the
        // abort conditions apply; else hold the dip.
        Posture::Dip if trigger => Posture::Pressured,
        Posture::Dip if dip_aborts => Posture::Free,
        Posture::Dip => Posture::Dip,
        // FREE: pressure first, then the offensive dip opportunity.
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
                // Hold ESCAPE (and the committed directive, and the heal
                // deferral) until the first impaired attacker breaks free.
                // The anchor and hysteresis floor survive — exiting back to
                // PRESSURED must not restart from scratch.
                state.escape_until = now + escape_window_secs.unwrap_or(0.0);
            }
            Posture::Dip => {
                state.dip_target = dip_entry;
                state.dip_until = now + movement.priest.dip_budget;
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

        // Dip → Free is always an abort (DipComplete exits go through the
        // dip-cast path in decide_priest_action). Stop the walk and trace it;
        // free_tick re-commits the formation directive below.
        if prev == Posture::Dip && next == Posture::Free {
            commands.entity(entity).remove::<MovementDirective>();
            if let Some(mut b) = start_movement_event(decision_trace, ctx) {
                b.transition(
                    TracePosture::Dip,
                    TracePosture::Free,
                    MovementTrigger::DipAbort,
                    MovementGoalKind::Entity,
                );
                b.finish();
            }
        }
    }

    let mut scream_dip = ScreamDipPlan::Rotation;
    match next {
        Posture::Escape => escape_tick(
            commands, entity, my_pos, ctx, state, directive, shared,
            &movement.priest.weights, decision_trace, transitioned, prev,
        ),
        Posture::Pressured => pressured_tick(
            commands, entity, combatant, my_pos, ctx, state, directive, movement, now,
            decision_trace, transitioned, prev,
        ),
        Posture::Dip => {
            scream_dip = priest_dip_tick(
                commands, abilities, entity, my_pos, ctx, state, directive, now,
                decision_trace, transitioned, prev,
            );
        }
        _ => free_tick(
            commands, entity, combatant, my_pos, ctx, state, directive, movement, now,
            decision_trace, transitioned, prev,
        ),
    }

    if needs_insert {
        commands.entity(entity).try_insert(*state);
    }

    // Cast-vs-move urgency: a live ESCAPE window OR a DIP defers non-critical
    // movement-locking casts (an undeferred heal mid-dip would stall the walk).
    let escape_defer = if matches!(state.posture, Posture::Escape | Posture::Dip) {
        Some(shared.urgency_hp_threshold)
    } else {
        None
    };

    PriestMovementPlan {
        escape_defer,
        scream_dip,
    }
}

/// PRESSURED tick: thin wrapper over [`healer_pressured_tick_shared`] with the
/// Priest's scorer weights, the kill target as the wand-pull source, and no
/// retreat band (`fallback_range = None` — the Priest scores a repulsion step
/// every re-evaluation rather than parking at a band like the Paladin).
#[allow(clippy::too_many_arguments)]
fn pressured_tick(
    commands: &mut Commands,
    entity: Entity,
    combatant: &Combatant,
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
    healer_pressured_tick_shared(
        commands,
        entity,
        my_pos,
        ctx,
        state,
        directive,
        &movement.shared,
        &movement.priest.weights,
        combatant.target,
        None,
        now,
        decision_trace,
        transitioned,
        prev,
    );
}

/// FREE tick: formation-point anchoring (R5). Degenerate case (no living
/// non-pet ally): NO directive — the legacy preferred_range pursuit governs
/// (AE4 — 1v1 behavior preserved).
#[allow(clippy::too_many_arguments)]
fn free_tick(
    commands: &mut Commands,
    entity: Entity,
    combatant: &Combatant,
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

    // ESCAPE → FREE is the window-expiry exit with no remaining threat —
    // trace it as EscapeWindowClosed, not PressuredExit.
    let exit_trigger = if prev == Posture::Escape {
        MovementTrigger::EscapeWindowClosed
    } else {
        MovementTrigger::PressuredExit
    };

    // Clear any directive committed under the prior posture on entry to FREE
    // (mirrors `paladin_free_tick`). A PRESSURED Direction walk carries a
    // ~1s TTL; without this it survives the transition and keeps driving the
    // Priest along the stale vector — the executor lets a live directive own
    // the frame — corrupting formation positioning and the commit-window
    // anti-zigzag guarantee. The normal path below re-issues a fresh Point
    // directive when the Priest is not already at the formation point; the
    // degenerate (no-ally) path leaves none so legacy pursuit governs.
    if transitioned {
        commands.entity(entity).remove::<MovementDirective>();
    }

    let Some(point) = compute_formation_point(entity, combatant, my_pos, ctx, movement) else {
        // DEGENERATE (R5/AE4): no formation directive; fall through to the
        // legacy ladder. Exit transitions still emit — goal_kind
        // Entity records "legacy target pursuit governs".
        if transitioned {
            if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
                builder.transition(
                    prev.into(),
                    TracePosture::Free,
                    exit_trigger,
                    MovementGoalKind::Entity,
                );
                builder.finish();
            }
        }
        return;
    };

    let point_xz = Vec2::new(point.x, point.z);
    let my_xz = Vec2::new(my_pos.x, my_pos.z);
    let moved = state
        .last_point
        .map_or(true, |lp| lp.distance(point_xz) > movement.priest.formation_shift_threshold);
    let near = my_xz.distance(point_xz) <= movement.priest.formation_deadzone;

    let issue = |commands: &mut Commands| {
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Point(point),
            expires: now + shared.directive_ttl,
            committed_until: now + shared.commit_window,
        });
    };

    if transitioned {
        // PRESSURED/ESCAPE → FREE: re-anchor to the formation point. Issue
        // unconditionally (even when already near it): the stale prior-posture
        // directive was just removed above, so when near, a Point directive's
        // arrival-hold is what parks the Priest at the backline — without it
        // legacy pursuit would drag the Priest forward into fresh pressure.
        issue(commands);
        state.last_point = Some(point_xz);
        if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
            builder.transition(
                prev.into(),
                TracePosture::Free,
                exit_trigger,
                MovementGoalKind::Point,
            );
            builder.finish();
        }
    } else if moved && !near {
        // Formation goal moved enough to re-commit (engaged-ally centroid
        // shifted) — FormationShift, within the same posture.
        issue(commands);
        state.last_point = Some(point_xz);
        if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
            builder.direction_change(
                TracePosture::Free,
                MovementTrigger::FormationShift,
                MovementGoalKind::Point,
            );
            builder.finish();
        }
    } else if !near
        && directive.map_or(true, |d| d.expires - now < movement.priest.directive_refresh_margin)
    {
        // Keep the standing walk alive (post-cast gaps, TTL expiry) without
        // re-scoring or emitting — refreshes are not decisions.
        issue(commands);
    }
    // `near` with no meaningful point move: the executor's Point-arrival
    // hold keeps us parked; the directive's TTL cleans it up.
}

/// FREE formation point (R5): centroid of living non-pet ENGAGED allies
/// (allies with an enemy target), excluding self; pre-contact (no ally
/// engaged) falls back to all living non-pet allies. Offset
/// `formation_offset` away from the nearest visible enemy ("behind the
/// line"), direction-blended toward arena center by `center_bias`, clamped
/// into wand range of the kill target (the FREE wand pull — an idle Priest
/// still drifts into wand range) and into arena bounds. `None` when no
/// living non-pet ally exists (degenerate case R5/AE4).
fn compute_formation_point(
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    movement: &MovementConfig,
) -> Option<Vec3> {
    let shared = &movement.shared;
    // Single pass over living non-pet allies (excluding self): accumulate the
    // all-ally and engaged-ally position sums + counts together, instead of
    // collecting two `Vec<&CombatantInfo>`. The formation anchors on engaged
    // allies (those with an enemy target) when any are engaged, else on all
    // allies (pre-contact). Bit-identical to the prior two-Vec version: the
    // centroid is the same group's mean, summed in `alive_allies()` order.
    let mut all_sum = Vec3::ZERO;
    let mut all_count = 0u32;
    let mut engaged_sum = Vec3::ZERO;
    let mut engaged_count = 0u32;
    for a in ctx.alive_allies() {
        if a.entity == entity {
            continue;
        }
        all_sum += a.position;
        all_count += 1;
        if a.target.is_some() {
            engaged_sum += a.position;
            engaged_count += 1;
        }
    }
    if all_count == 0 {
        return None;
    }
    let centroid = if engaged_count > 0 {
        engaged_sum / engaged_count as f32
    } else {
        all_sum / all_count as f32
    };

    // "Behind the line": offset away from the nearest visible enemy; with no
    // visible enemy (stealth pre-contact), away from arena center — trailing
    // the allies as they advance.
    let nearest_enemy = ctx
        .visible_enemies_within(entity, centroid, f32::MAX)
        .into_iter()
        .min_by(|a, b| {
            centroid
                .distance(a.position)
                .partial_cmp(&centroid.distance(b.position))
                .unwrap()
        });
    let away = match nearest_enemy {
        Some(e) => Vec2::new(centroid.x - e.position.x, centroid.z - e.position.z)
            .normalize_or_zero(),
        None => Vec2::new(centroid.x, centroid.z).normalize_or_zero(),
    };
    let to_center = Vec2::new(-centroid.x, -centroid.z).normalize_or_zero();
    let mut dir = (away * (1.0 - shared.center_bias) + to_center * shared.center_bias)
        .normalize_or_zero();
    if dir == Vec2::ZERO {
        dir = away;
    }
    let mut point = Vec3::new(
        centroid.x + dir.x * shared.formation_offset,
        my_pos.y,
        centroid.z + dir.y * shared.formation_offset,
    );

    // Wand-range pull: clamp the point to the wand-range boundary of the
    // kill target when it would sit outside it (weight 0 disables — the
    // Paladin has no wand).
    if movement.priest.weights.wand_pull > 0.0 {
        if let Some(target) = combatant
            .target
            .and_then(|t| ctx.combatants.get(&t))
            .filter(|i| i.is_alive)
        {
            let offset = Vec2::new(point.x - target.position.x, point.z - target.position.z);
            let dist = offset.length();
            if dist > shared.wand_range {
                let clamped = offset / dist * shared.wand_range;
                point.x = target.position.x + clamped.x;
                point.z = target.position.z + clamped.y;
            }
        }
    }

    Some(clamp_to_arena(point))
}
