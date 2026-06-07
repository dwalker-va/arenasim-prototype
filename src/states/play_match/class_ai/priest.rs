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
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::{
    calculate_cast_time, clamp_to_arena, compass_directions_16, score_directions,
    AnchorConstraint, ScorerInputs,
};
use crate::states::play_match::constants::GCD;
use crate::states::play_match::decision_trace::{
    ActorView, DecisionEventBuilder, DecisionTrace, MovementEventBuilder, MovementGoalKind,
    MovementTrigger, Posture as TracePosture, RejectionReason,
};
use crate::states::play_match::movement_config::MovementConfig;
use crate::states::play_match::utils::log_ability_use;

use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};

use super::{CombatContext, CombatantInfo};

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
    escape_defer: Option<f32>,
    decision_trace: &mut DecisionTrace,
) -> bool {
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

    // Priority 3: Power Word: Shield
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

/// ESCAPE window math (R7), pure for unit testing.
///
/// `proximate_cc_remaining` holds, per threat within the danger radius, the
/// remaining Root/Stun/Incapacitate duration (`attacker_escape_window`) or
/// `None` for an unimpaired threat. Rules:
///
/// - **Multi-attacker rule:** a single unimpaired proximate threat voids the
///   window (`None` anywhere → no ESCAPE).
/// - **Empty set:** no proximate threat → nothing to escape from → no window.
/// - **Window duration:** min over the impaired threats of their remaining CC
///   (the first attacker to break free ends the useful window).
/// - **Sub-cutoff rule (slow-adjusted):** the window is only worth a heal
///   deferral if it buys real distance. Distance gained ≈ window ×
///   base_speed × slow_multiplier (see [`escape_distance_gained`]), so the
///   slow-adjusted *effective* window is `window × slow_multiplier`. If that
///   falls below `min_window` (config `shared.escape_min_window`, calibrated
///   at full speed), do not enter ESCAPE — a 50%-slowed Priest needs twice
///   the CC time to gain the same separation.
///
/// Returns the RAW window duration in seconds (the directive/posture hold
/// time — the slowed Priest still escapes for the full CC duration once the
/// window is worth entering).
pub fn escape_window(
    proximate_cc_remaining: &[Option<f32>],
    slow_multiplier: f32,
    min_window: f32,
) -> Option<f32> {
    if proximate_cc_remaining.is_empty() {
        return None;
    }
    let mut window = f32::MAX;
    for cc in proximate_cc_remaining {
        match cc {
            Some(remaining) => window = window.min(*remaining),
            // Multi-attacker rule: one free proximate threat voids the window.
            None => return None,
        }
    }
    // Sub-cutoff rule, slow-adjusted: effective window = raw × slow multiplier.
    if window * slow_multiplier < min_window {
        return None;
    }
    Some(window)
}

/// Distance gained over an ESCAPE window: `window × base_speed ×
/// slow_multiplier`. A 50% slow (`slow_multiplier = 0.5`) halves the
/// effective escape distance — this is the relationship the sub-cutoff rule
/// in [`escape_window`] is built on.
pub fn escape_distance_gained(window: f32, base_speed: f32, slow_multiplier: f32) -> f32 {
    window * base_speed * slow_multiplier
}

/// Distance the FREE formation point must move (XZ units) before the
/// directive is re-targeted and a `FormationShift` trace event fires.
const FORMATION_SHIFT_THRESHOLD: f32 = 3.0;
/// FREE deadzone: no Point directive is issued when the Priest is already
/// this close to the formation point (prevents micro-shuffling).
const FORMATION_DEADZONE: f32 = 1.5;
/// Distance ahead at which the position scorer evaluates candidate steps.
const SCORER_LOOKAHEAD: f32 = 2.0;
/// Refresh the standing FREE directive when its remaining TTL drops below
/// this (keeps a walk alive across decide ticks without re-scoring or
/// emitting — refreshes are not decisions).
const DIRECTIVE_REFRESH_MARGIN: f32 = 0.25;

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
    posture: Option<&mut HealerPosture>,
    directive: Option<&MovementDirective>,
    movement: &MovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
) -> Option<f32> {
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

    // --- PRESSURED compound trigger (R6) ---
    // Targeted by a VISIBLE enemy (enemies_targeting is stealth-filtered —
    // AE2: no pre-dodging invisible Rogues; pets included) AND a proximity /
    // intent condition: within the danger radius, or a melee-class / pet /
    // closing threat within the intent radius. A distant caster holding
    // position while targeting me does NOT flip the posture (AE5), and
    // neither does a melee targeting me from across the arena — pressure
    // requires the threat to be near enough that intent matters.
    let trigger = ctx.enemies_targeting(entity).iter().any(|t| {
        let distance = my_pos.distance(t.position);
        distance <= shared.danger_radius
            || (distance <= shared.threat_intent_radius
                && (t.is_pet || t.class.is_melee() || ctx.is_closing(t.entity, entity)))
    });

    let prev = state.posture;

    // --- ESCAPE entry window (R7) --- evaluated only while currently
    // PRESSURED with the trigger still live: every visible threat within the
    // danger radius must be movement-impaired (Root/Stun/Incapacitate — Fear
    // excluded by `attacker_escape_window`: a feared attacker self-solves,
    // and a Fear-only window must NOT trigger ESCAPE). One unimpaired
    // proximate threat voids the window; sub-cutoff windows (slow-adjusted)
    // are ignored. See `escape_window` for the full rule set.
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
        // DIP is Paladin-only (U8); Priest FREE transitions on the trigger.
        _ if trigger => Posture::Pressured,
        _ => Posture::Free,
    };

    let transitioned = next != prev;
    if transitioned {
        state.posture = next;
        state.since = now;
        state.last_direction = None;
        state.last_point = None;
        match next {
            Posture::Pressured => state.hold_until = now + shared.pressured_hold,
            Posture::Escape => {
                // Hold ESCAPE (and the committed directive, and the heal
                // deferral) until the first impaired attacker breaks free.
                // The anchor and hysteresis floor survive — exiting back to
                // PRESSURED must not restart from scratch.
                state.escape_until = now + escape_window_secs.unwrap_or(0.0);
            }
            _ => {
                state.hold_until = 0.0;
                state.anchor = None;
            }
        }
    }

    match next {
        Posture::Escape => escape_tick(
            commands, entity, my_pos, ctx, state, directive, movement,
            decision_trace, transitioned, prev,
        ),
        Posture::Pressured => pressured_tick(
            commands, entity, combatant, my_pos, ctx, state, directive, movement, now,
            decision_trace, transitioned, prev,
        ),
        _ => free_tick(
            commands, entity, combatant, my_pos, ctx, state, directive, movement, now,
            decision_trace, transitioned, prev,
        ),
    }

    if needs_insert {
        commands.entity(entity).try_insert(*state);
    }

    // Cast-vs-move urgency: live ESCAPE window → defer non-critical casts.
    if state.posture == Posture::Escape {
        Some(shared.urgency_hp_threshold)
    } else {
        None
    }
}

/// ESCAPE tick (R7): on entry, score one direction with attacker repulsion
/// dominant — threats are the impaired proximate attackers; the formation
/// and wand pulls are OFF so repulsion is the only directional soft term,
/// while the ally-anchor heal-range constraint and the boundary/corner
/// penalties stay ACTIVE (escapes bend along walls instead of pinning into
/// them, and never leave heal range of the anchor). The directive is
/// committed for the whole window (`expires == committed_until ==
/// escape_until`): mid-window ticks re-issue defensively but never re-score
/// or re-emit.
#[allow(clippy::too_many_arguments)]
fn escape_tick(
    commands: &mut Commands,
    entity: Entity,
    my_pos: Vec3,
    ctx: &CombatContext,
    state: &mut HealerPosture,
    directive: Option<&MovementDirective>,
    movement: &MovementConfig,
    decision_trace: &mut DecisionTrace,
    transitioned: bool,
    prev: Posture,
) {
    let shared = &movement.shared;

    if !transitioned {
        // Committed mid-window: keep the directive alive if it somehow died
        // (its expiry equals the window end, so this is defensive only) —
        // refreshes are not decisions, so no re-score and no trace event.
        if directive.is_none() {
            if let Some(dir) = state.last_direction {
                commands.entity(entity).try_insert(MovementDirective {
                    goal: MovementGoal::Direction(dir),
                    expires: state.escape_until,
                    committed_until: state.escape_until,
                });
            }
        }
        return;
    }

    // Same sticky anchor as PRESSURED — the heal-range constraint stays hard
    // during the escape (a window must never carry the Priest out of range
    // of the ally it exists to keep healing).
    let anchor_info = select_sticky_anchor(entity, ctx, state, shared);

    // Threats: the impaired proximate attackers (ESCAPE entry guarantees
    // every visible enemy inside the danger radius is impaired right now).
    // BTreeMap for deterministic scorer input order.
    let mut threat_positions: std::collections::BTreeMap<Entity, Vec3> = Default::default();
    for t in ctx.visible_enemies_within(entity, my_pos, shared.danger_radius) {
        threat_positions.insert(t.entity, t.position);
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
        // No wand pull during an escape — repulsion must dominate, and a
        // pull toward any enemy would shrink the separation the window buys.
        wand_target: None,
        wand_range: shared.wand_range,
        committed_direction: None,
    };
    let chosen = score_directions(&compass_directions_16(), &inputs, &movement.priest.weights);
    if chosen == Vec2::ZERO {
        return; // defensive — 16 candidates always yield a direction
    }

    commands.entity(entity).try_insert(MovementDirective {
        goal: MovementGoal::Direction(chosen),
        expires: state.escape_until,
        committed_until: state.escape_until,
    });
    state.last_direction = Some(chosen);

    if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
        builder.transition(
            prev.into(),
            TracePosture::Escape,
            MovementTrigger::EscapeWindowOpen,
            MovementGoalKind::Direction,
        );
        builder.chosen_direction([chosen.x, chosen.y]);
        builder.finish();
    }
}

/// Sticky anchor ally (R6): most-injured living non-pet ally, excluding
/// self (the constraint keeps US within heal range of THEM). Switching
/// requires the candidate to be more injured than the current anchor by
/// `anchor_switch_margin`, so two similarly-injured allies don't flap the
/// constraint region tick to tick. BTree iteration + strict `<` keeps
/// ties deterministic. Shared by PRESSURED and ESCAPE (the escape direction
/// honors the same heal-range constraint). Updates `state.anchor`.
fn select_sticky_anchor<'c>(
    entity: Entity,
    ctx: &'c CombatContext,
    state: &mut HealerPosture,
    shared: &crate::states::play_match::movement_config::SharedMovementConfig,
) -> Option<&'c CombatantInfo> {
    let candidate = ctx
        .alive_allies()
        .into_iter()
        .filter(|a| a.entity != entity)
        .min_by(|a, b| a.health_pct().partial_cmp(&b.health_pct()).unwrap());
    let current = state
        .anchor
        .and_then(|a| ctx.combatants.get(&a))
        .filter(|i| i.is_alive && !i.is_pet);
    let anchor_info: Option<&CombatantInfo> = match (current, candidate) {
        (Some(cur), Some(cand))
            if cand.entity != cur.entity
                && cand.health_pct() + shared.anchor_switch_margin < cur.health_pct() =>
        {
            Some(cand)
        }
        (Some(cur), _) => Some(cur),
        (None, cand) => cand,
    };
    state.anchor = anchor_info.map(|i| i.entity);
    anchor_info
}

/// PRESSURED tick: sticky anchor selection, hard commitment window, scored
/// direction, directive issuance, transition/direction-change trace events.
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
    let shared = &movement.shared;

    let anchor_info = select_sticky_anchor(entity, ctx, state, shared);

    // Hard commitment window (R11): re-evaluation happens only once the
    // committed window lapses (or the directive died — e.g. expired across a
    // Flash Heal cast). The scorer's commitment bonus applies only AT
    // re-evaluation; the two governors never stack.
    let window_open =
        directive.map_or(false, |d| now < d.committed_until && now < d.expires);
    if window_open && !transitioned {
        return;
    }

    // Threat set: visible enemies targeting me + any visible enemy inside
    // the danger radius (an enemy in my face is a threat even while it
    // targets someone else). BTreeMap dedupes in deterministic order.
    let mut threat_positions: std::collections::BTreeMap<Entity, Vec3> = Default::default();
    for t in ctx.enemies_targeting(entity) {
        threat_positions.insert(t.entity, t.position);
    }
    for t in ctx.visible_enemies_within(entity, my_pos, shared.danger_radius) {
        threat_positions.insert(t.entity, t.position);
    }

    // Wand pull while PRESSURED — but never toward an enemy that is itself
    // in the threat set: drifting toward your own attacker would cancel the
    // repulsion term at mid range and park the Priest at a standoff distance
    // instead of escaping (observed in the statue probe before this guard).
    let wand_target = combatant
        .target
        .filter(|t| !threat_positions.contains_key(t))
        .and_then(|t| ctx.combatants.get(&t))
        .filter(|i| i.is_alive)
        .map(|i| i.position);

    let inputs = ScorerInputs {
        my_pos,
        lookahead: SCORER_LOOKAHEAD,
        threats: threat_positions.into_values().collect(),
        anchor: anchor_info.map(|i| AnchorConstraint {
            pos: i.position,
            heal_range: shared.heal_range,
        }),
        formation_point: None,
        wand_target,
        wand_range: shared.wand_range,
        committed_direction: state.last_direction,
    };
    let chosen = score_directions(&compass_directions_16(), &inputs, &movement.priest.weights);
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

    // Trace (R3): posture transitions and committed direction CHANGES only.
    if transitioned || direction_changed {
        if let Some(mut builder) = start_movement_event(decision_trace, ctx) {
            if transitioned {
                // ESCAPE → PRESSURED is the window-expiry exit, not a fresh
                // pressure onset — trace it as EscapeWindowClosed.
                let trigger = if prev == Posture::Escape {
                    MovementTrigger::EscapeWindowClosed
                } else {
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
        .map_or(true, |lp| lp.distance(point_xz) > FORMATION_SHIFT_THRESHOLD);
    let near = my_xz.distance(point_xz) <= FORMATION_DEADZONE;

    let issue = |commands: &mut Commands| {
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Point(point),
            expires: now + shared.directive_ttl,
            committed_until: now + shared.commit_window,
        });
    };

    if transitioned {
        // PRESSURED → FREE: re-anchor to the formation immediately.
        if !near {
            issue(commands);
        }
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
    } else if !near && directive.map_or(true, |d| d.expires - now < DIRECTIVE_REFRESH_MARGIN) {
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
    let allies: Vec<&CombatantInfo> = ctx
        .alive_allies()
        .into_iter()
        .filter(|a| a.entity != entity)
        .collect();
    if allies.is_empty() {
        return None;
    }
    let engaged: Vec<&CombatantInfo> = allies
        .iter()
        .copied()
        .filter(|a| a.target.is_some())
        .collect();
    let group: &[&CombatantInfo] = if engaged.is_empty() { &allies } else { &engaged };
    let centroid =
        group.iter().fold(Vec3::ZERO, |acc, a| acc + a.position) / group.len() as f32;

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

/// Start a `movement_decision` builder for the current actor. `None` only
/// when the snapshot lacks self (defensive — shouldn't happen in dispatch).
fn start_movement_event<'t>(
    decision_trace: &'t mut DecisionTrace,
    ctx: &CombatContext,
) -> Option<MovementEventBuilder<'t>> {
    let actor = ActorView::from_info(ctx.self_info()?);
    Some(decision_trace.start_movement_decision(actor, None))
}
