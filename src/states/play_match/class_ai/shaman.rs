//! Shaman AI Module
//!
//! Handles AI decision-making for the Shaman class — a mana ranged
//! caster-healer whose identity is offensive tempo (Lightning Bolt pressure,
//! Purge, Wind Shear) backed by four element totems and an opportunistic
//! Lesser Healing Wave.
//!
//! ## Status
//! UNIT 3. Totem maintenance is wired: the AI drops and refreshes its four
//! element totems (Air/Water/Earth/Fire) via `maintain_totems`, which
//! `decide_shaman_action` calls so totems land in matches now. The full healer
//! posture machine + offensive rotation (Wind Shear / Purge / Lightning Bolt /
//! Frost Shock / Lesser Healing Wave) arrive in U6, which will reuse
//! `maintain_totems` as the rotation's priority-1 step.
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;
use std::collections::BTreeSet;

use crate::combat::log::{CombatLog, CombatLogEventType};
use crate::states::play_match::abilities::{AbilityType, SpellSchool};
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::combat_core::{calculate_cast_time, clamp_to_arena};
use crate::states::play_match::components::*;
use crate::states::play_match::constants::*;
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, MovementGoalKind, MovementTrigger,
    Posture as TracePosture, RejectionReason,
};
use crate::states::play_match::movement_config::MovementConfig;

use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};
use super::healer_postures::{
    compound_pressure_trigger, escape_tick, escape_window_from, healer_pressured_tick_shared,
    start_movement_event,
};
use super::super::utils::log_ability_use;
use super::CombatContext;

/// Emergency heal trigger — a teammate below this HP fraction is healed before
/// any offense (critical, never deferred during an escape).
const SHAMAN_EMERGENCY_HP: f32 = 0.40;
/// Sustain heal trigger — a teammate below this HP fraction is topped off only
/// when the Shaman isn't fleeing (deferred during an ESCAPE window).
const SHAMAN_SUSTAIN_HP: f32 = 0.70;
/// Mana floor below which the Shaman stops REFRESHING expiring totems (initial
/// drops are still allowed) so totem upkeep doesn't starve Lightning Bolt /
/// Frost Shock / heals.
const SHAMAN_TOTEM_REFRESH_MANA_FLOOR: f32 = 60.0;

/// Per-tick output of [`evaluate_shaman_posture`] (mirrors `PriestMovementPlan`
/// minus the dip: the Shaman has no Hammer-of-Justice / Psychic-Scream dip).
pub struct ShamanMovementPlan {
    /// `Some(urgency_hp_threshold)` while an ESCAPE window is live: the heal
    /// ladder defers non-critical movement-locking casts (Lesser Healing Wave
    /// for healthy targets, Lightning Bolt) for the window.
    pub escape_defer: Option<f32>,
    /// The live PRESSURED trigger this tick (`compound_pressure_trigger`),
    /// kept for parity with the Priest plan.
    pub pressured: bool,
}

impl Default for ShamanMovementPlan {
    fn default() -> Self {
        Self { escape_defer: None, pressured: false }
    }
}

/// Shaman AI: decides and executes abilities for a Shaman combatant.
///
/// U6: full GCD-gated rotation in priority order. `totem_durations` is the
/// Shaman's per-element live totem state (`[remaining; 4]` indexed by
/// `TotemElement::index()`, `0.0` = absent), supplied by `decide_abilities`.
/// `plan` carries the posture machine's ESCAPE deferral; `movement` supplies
/// the shared heal-range constraint.
///
/// Returns `true` if an action was taken this frame.
pub fn decide_shaman_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    totem_durations: &[f32; 4],
    plan: &ShamanMovementPlan,
    movement: &MovementConfig,
    decision_trace: &mut DecisionTrace,
) -> bool {
    let escape_defer = plan.escape_defer;

    // GCD gate — at most one ability per GCD.
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, combatant.target, my_pos) else {
        return false;
    };

    // P1: emergency heal — a dying ally is topped off before ANYTHING else
    // (including totem maintenance: never let an ally die to refresh a buff
    // totem). Critical: never deferred for an escape window.
    if try_lesser_healing_wave(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        SHAMAN_EMERGENCY_HP, None, movement, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // P2: Frost Shock — instant Frost nuke + slow, used as a peel against a
    // melee/pet attacking the Shaman or a low-HP ally.
    if try_frost_shock(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // P3: keep the four element totems up (consumes the GCD when one drops).
    // Refreshes are deferred below a mana floor so totems don't starve offense;
    // initial drops are always allowed.
    if maintain_totems(
        commands, combat_log, abilities, entity, combatant, my_pos, totem_durations, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // P4: Lightning Bolt — cast-time filler nuke on the kill target. Deferred
    // while fleeing (don't hardcast mid-escape).
    if try_lightning_bolt(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        escape_defer, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // P5: sustain heal — top off an injured (non-emergency) ally. Lighter gate
    // than the Priest: the Shaman is offense-slanted, so it only heals when
    // needed. Deferred for healthy-ish targets while an escape window is live.
    if try_lesser_healing_wave(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx,
        SHAMAN_SUSTAIN_HP, escape_defer, movement, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // P6: Purge — strip a beneficial aura off an enemy (prefers the healer).
    if super::try_purge_enemy(
        commands, combat_log, abilities, entity, combatant, my_pos, auras, ctx, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    builder.finish();
    false
}

/// Try to cast Lesser Healing Wave on the lowest-HP ally below `hp_threshold`
/// (within shared heal range). When `escape_defer` is `Some(threshold)` and the
/// would-be target's HP fraction is ABOVE it, the heal is deferred — the cast
/// locks movement and would freeze the Shaman mid-escape (mirrors the Priest's
/// Flash Heal urgency rule). Mana is consumed at cast completion by the casting
/// system (this only sets the GCD + inserts `CastingState`).
fn try_lesser_healing_wave(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    hp_threshold: f32,
    escape_defer: Option<f32>,
    movement: &MovementConfig,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::LesserHealingWave;
    let def = abilities.get_unchecked(&ability);

    let Some(target_info) =
        ctx.lowest_health_ally_below(hp_threshold, movement.shared.heal_range, my_pos)
    else {
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
    if !pre_cast_ok(ability, def, combatant, my_pos, auras, Some((heal_target, target_pos)), ctx, opts) {
        builder.reject(
            ability,
            classify_pre_cast_failure(ability, def, combatant, my_pos, auras, Some((heal_target, target_pos)), ctx, opts),
        );
        return false;
    }

    builder.choose(ability, Some(heal_target), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);
    commands.entity(entity).insert(CastingState::new(ability, heal_target, cast_time));

    let target_tuple = ctx.combatants.get(&heal_target).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, target_tuple, "begins casting");

    true
}

/// Choose a Frost Shock target: a melee/pet enemy in range that is attacking
/// the Shaman, or attacking a low-HP ally (a peel) — nearest first; else the
/// kill target if it's in range. Deterministic (BTree iteration + distance
/// tie-break by entity), no RNG.
fn frost_shock_target(
    ctx: &CombatContext,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    range: f32,
) -> Option<Entity> {
    // Allies (excluding self) currently in trouble — peel their attacker.
    let low_allies: BTreeSet<Entity> = ctx
        .alive_allies()
        .into_iter()
        .filter(|a| a.entity != entity && a.health_pct() < SHAMAN_EMERGENCY_HP)
        .map(|a| a.entity)
        .collect();

    // Peel candidate: a proximate melee/pet threat on me or a low-HP ally.
    let peel = ctx
        .combatants
        .iter()
        .filter(|(_, info)| info.team != combatant.team && info.is_alive)
        .filter(|(_, info)| info.is_pet || info.class.is_melee())
        .filter(|(e, info)| {
            my_pos.distance(info.position) <= range
                && !ctx.entity_is_immune(**e)
                && (info.target == Some(entity)
                    || info.target.map_or(false, |t| low_allies.contains(&t)))
        })
        .min_by(|(ea, a), (eb, b)| {
            my_pos
                .distance(a.position)
                .partial_cmp(&my_pos.distance(b.position))
                .unwrap()
                .then(ea.cmp(eb))
        })
        .map(|(e, _)| *e);
    if peel.is_some() {
        return peel;
    }

    // Fallback: the kill target, if alive, in range, and not immune.
    combatant.target.filter(|t| {
        ctx.combatants.get(t).map_or(false, |i| {
            i.is_alive && my_pos.distance(i.position) <= range && !ctx.entity_is_immune(*t)
        })
    })
}

/// Try to cast Frost Shock — instant Frost nuke that applies a non-breaking
/// slow (a peel). Routed through `CastingState` with the ability's 0.0 cast
/// time so the generic completion path applies BOTH the damage (correct school
/// + spell-power scaling) and the slow aura (`def.applies_aura`) — the same
/// generic path Lightning Bolt and Lesser Healing Wave use. The 6s cooldown
/// (enforced by `pre_cast_ok`) prevents spam; mana is consumed at completion.
fn try_frost_shock(
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
    let ability = AbilityType::FrostShock;
    let def = abilities.get_unchecked(&ability);

    let Some(target_entity) = frost_shock_target(ctx, entity, combatant, my_pos, def.range) else {
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
    if !pre_cast_ok(ability, def, combatant, my_pos, auras, Some((target_entity, target_pos)), ctx, opts) {
        builder.reject(
            ability,
            classify_pre_cast_failure(ability, def, combatant, my_pos, auras, Some((target_entity, target_pos)), ctx, opts),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), true);

    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras); // 0.0 — completes immediately
    commands.entity(entity).insert(CastingState::new(ability, target_entity, cast_time));

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, target_tuple, "casts");

    true
}

/// Try to cast Lightning Bolt — the Shaman's cast-time filler nuke on the kill
/// target (modeled on the Mage's Frostbolt). Deferred outright while an ESCAPE
/// window is live (`escape_defer` is `Some`): a hardcast would freeze the
/// Shaman mid-escape. Mana is consumed at cast completion by the casting system.
fn try_lightning_bolt(
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
    let ability = AbilityType::LightningBolt;
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
        check_target_immune: true,
        check_friendly_cc: true,
        ..Default::default()
    };
    if !pre_cast_ok(ability, def, combatant, my_pos, auras, Some((target_entity, target_pos)), ctx, opts) {
        builder.reject(
            ability,
            classify_pre_cast_failure(ability, def, combatant, my_pos, auras, Some((target_entity, target_pos)), ctx, opts),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);
    commands.entity(entity).insert(CastingState::new(ability, target_entity, cast_time));

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| (info.team, info.class));
    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, target_tuple, "begins casting");

    true
}

// ==============================================================================
// Totem maintenance (U3)
// ==============================================================================

/// Drop or refresh the four element totems. For each element whose totem is
/// missing or about to expire (`< TOTEM_REFRESH_THRESHOLD` seconds remaining),
/// attempt its cast. Returns `true` on the first totem dropped this tick (each
/// drop consumes the GCD, so only one lands per call). Healthy totems are
/// skipped silently; cast attempts that fail (cooldown / mana) emit a trace
/// rejection and fall through to the next element.
///
/// U6 will call this as priority P1 of the full Shaman rotation.
pub(super) fn maintain_totems(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    totem_durations: &[f32; 4],
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    for element in TotemElement::ALL {
        let remaining = totem_durations[element.index()];
        if remaining >= TOTEM_REFRESH_THRESHOLD {
            continue; // totem healthy — leave it
        }

        // A refresh (totem present but expiring) is deferred below a mana floor
        // so totem upkeep never starves the Shaman's offense; an initial drop
        // (totem absent, `remaining == 0.0`) is always allowed.
        let is_refresh = remaining > 0.0;
        if is_refresh && combatant.current_mana < SHAMAN_TOTEM_REFRESH_MANA_FLOOR {
            continue;
        }

        let cast = match element {
            TotemElement::Air => try_air_totem(commands, combat_log, abilities, entity, combatant, my_pos, builder),
            TotemElement::Water => try_water_totem(commands, combat_log, abilities, entity, combatant, my_pos, builder),
            TotemElement::Earth => try_earth_totem(commands, combat_log, abilities, entity, combatant, my_pos, builder),
            TotemElement::Fire => try_fire_totem(commands, combat_log, abilities, entity, combatant, my_pos, builder),
        };
        if cast {
            return true;
        }
    }
    false
}

/// Per-element ability/buff mapping. MODEST magnitudes — real balance is a later
/// unit. Mirrors the `TotemElement -> buff` table documented in the U3 spec.
fn totem_spec(element: TotemElement) -> (AbilityType, AuraType, f32, SpellSchool) {
    match element {
        // Windfury Totem — empowers melee allies' auto-attacks (proc chance 0..1).
        TotemElement::Air => (AbilityType::AirTotem, AuraType::WindfuryBuff, 0.12, SpellSchool::Nature),
        // Healing Stream Totem — periodic ally heal (per-tick amount).
        TotemElement::Water => (AbilityType::WaterTotem, AuraType::HealingOverTime, 8.0, SpellSchool::Nature),
        // Strength of Earth Totem — flat attack power. Tempered alongside
        // Flametongue (SP) so physical partners (Warrior/Rogue/Hunter) don't
        // win the damage race against a Priest-healed mirror by totem buffs alone.
        TotemElement::Earth => (AbilityType::EarthTotem, AuraType::AttackPowerIncrease, 15.0, SpellSchool::Nature),
        // Flametongue Totem — flat spell power.
        TotemElement::Fire => (AbilityType::FireTotem, AuraType::SpellPowerIncrease, 18.0, SpellSchool::Fire),
    }
}

/// Deterministic per-element horizontal offset so the four totems fan out
/// around the Shaman's feet (compass directions at 0/90/180/270 degrees by
/// element index). No RNG — required for seeded-replay determinism.
fn totem_spacing_offset(element: TotemElement) -> Vec3 {
    let angle = element.index() as f32 * std::f32::consts::FRAC_PI_2;
    Vec3::new(angle.cos(), 0.0, angle.sin()) * TOTEM_SPACING_OFFSET
}

/// Cast-and-drop a single totem. Modeled on the Hunter's Frost Trap cast but
/// with NO projectile: gate on cooldown + mana + GCD, emit the trace
/// choose/reject, deduct mana, start the cooldown + GCD, and spawn the `Totem`
/// entity at the Shaman's feet plus the element's spacing offset.
fn try_totem(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    element: TotemElement,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let (ability, aura_type, magnitude, spell_school) = totem_spec(element);

    let Some(def) = abilities.get(&ability) else { return false };

    if let Some(remaining) = combatant.ability_cooldowns.get(&ability) {
        builder.reject(ability, RejectionReason::OnCooldown { remaining: *remaining });
        return false;
    }
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

    builder.choose(ability, None, true);

    let team = combatant.team;
    let drop = my_pos + totem_spacing_offset(element);
    let drop = Vec3::new(drop.x, 0.0, drop.z); // totems sit on the ground

    commands.spawn((
        Transform::from_translation(drop),
        Totem {
            owner_team: team,
            owner: entity,
            element,
            radius: TOTEM_RADIUS,
            duration_remaining: TOTEM_DURATION,
            aura_type,
            magnitude,
            spell_school,
        },
        PlayMatchEntity,
    ));

    combatant.current_mana -= def.mana_cost;
    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;

    combat_log.log(
        CombatLogEventType::Buff,
        format!("[TOTEM] Team {} Shaman drops {}", team, element.buff_name()),
    );
    log_ability_use(combat_log, team, combatant.class, &def.name, None, "drops");

    true
}

/// Windfury Totem (Air).
fn try_air_totem(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    try_totem(commands, combat_log, abilities, entity, combatant, my_pos, TotemElement::Air, builder)
}

/// Healing Stream Totem (Water).
fn try_water_totem(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    try_totem(commands, combat_log, abilities, entity, combatant, my_pos, TotemElement::Water, builder)
}

/// Strength of Earth Totem (Earth).
fn try_earth_totem(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    try_totem(commands, combat_log, abilities, entity, combatant, my_pos, TotemElement::Earth, builder)
}

/// Flametongue Totem (Fire).
fn try_fire_totem(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    try_totem(commands, combat_log, abilities, entity, combatant, my_pos, TotemElement::Fire, builder)
}

// ============================================================================
// Posture evaluation (healer movement AI — FREE/PRESSURED/ESCAPE)
// ============================================================================
//
// A stripped copy of the Priest posture machine (`evaluate_priest_posture` /
// `pressured_tick` / `free_tick` / `compute_formation_point`) with ALL Dip
// branches removed — the Shaman has no offensive dip. It reads `movement.shaman`
// (its own offense-slanted block) instead of `movement.priest`, and uses the
// kill target as the FREE wand-pull source (the Shaman drifts toward Lightning
// Bolt range of its kill target).

/// Evaluate the Shaman's movement posture (FREE/PRESSURED/ESCAPE) and
/// issue/refresh a [`MovementDirective`]. Runs at the top of the Shaman's
/// decide tick (BEFORE the GCD short-circuit — the GCD locks casts, not legs),
/// and only after gates open (caller gates on `countdown.gates_opened`).
///
/// Returns a [`ShamanMovementPlan`] whose `escape_defer` is
/// `Some(urgency_hp_threshold)` while an ESCAPE window is live — the rotation
/// defers non-critical movement-locking casts for the window.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_shaman_posture(
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
) -> ShamanMovementPlan {
    let mut local = HealerPosture::new(now);
    let needs_insert = posture.is_none();
    let state: &mut HealerPosture = match posture {
        Some(p) => p,
        None => &mut local,
    };

    let shared = &movement.shared;

    // PRESSURED compound trigger (shared with the Priest/Paladin).
    let trigger = compound_pressure_trigger(entity, my_pos, ctx, shared);

    let prev = state.posture;

    // ESCAPE entry window: only while PRESSURED with the trigger still live and
    // every proximate visible threat movement-impaired.
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

    let next = match prev {
        Posture::Escape if now < state.escape_until => Posture::Escape,
        Posture::Escape if trigger => Posture::Pressured,
        Posture::Escape => Posture::Free,
        Posture::Pressured if !trigger && now >= state.hold_until => Posture::Free,
        Posture::Pressured if escape_window_secs.is_some() => Posture::Escape,
        Posture::Pressured => Posture::Pressured,
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
            Posture::Pressured => {
                state.hold_until = now + shared.pressured_hold;
            }
            Posture::Escape => {
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
            commands, entity, my_pos, ctx, state, directive, shared,
            &movement.shaman.weights, decision_trace, transitioned, prev,
        ),
        Posture::Pressured => shaman_pressured_tick(
            commands, entity, combatant, my_pos, ctx, state, directive, movement, now,
            decision_trace, transitioned, prev,
        ),
        _ => shaman_free_tick(
            commands, entity, combatant, my_pos, ctx, state, directive, movement, now,
            decision_trace, transitioned, prev,
        ),
    }

    if needs_insert {
        commands.entity(entity).try_insert(*state);
    }

    let escape_defer = if state.posture == Posture::Escape {
        Some(shared.urgency_hp_threshold)
    } else {
        None
    };

    ShamanMovementPlan {
        escape_defer,
        pressured: trigger,
    }
}

/// PRESSURED tick: thin wrapper over [`healer_pressured_tick_shared`] with the
/// Shaman's scorer weights, the kill target as the wand-pull (Lightning-Bolt
/// range) source, and no retreat band (`fallback_range = None`).
#[allow(clippy::too_many_arguments)]
fn shaman_pressured_tick(
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
        &movement.shaman.weights,
        combatant.target,
        None,
        now,
        decision_trace,
        transitioned,
        prev,
    );
}

/// FREE tick: formation-point anchoring. Degenerate case (no living non-pet
/// ally): NO directive — the legacy preferred_range pursuit governs.
#[allow(clippy::too_many_arguments)]
fn shaman_free_tick(
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

    let exit_trigger = if prev == Posture::Escape {
        MovementTrigger::EscapeWindowClosed
    } else {
        MovementTrigger::PressuredExit
    };

    if transitioned {
        commands.entity(entity).remove::<MovementDirective>();
    }

    let Some(point) = compute_formation_point(entity, combatant, my_pos, ctx, movement) else {
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
        .map_or(true, |lp| lp.distance(point_xz) > movement.shaman.formation_shift_threshold);
    let near = my_xz.distance(point_xz) <= movement.shaman.formation_deadzone;

    let issue = |commands: &mut Commands| {
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Point(point),
            expires: now + shared.directive_ttl,
            committed_until: now + shared.commit_window,
        });
    };

    if transitioned {
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
        && directive.map_or(true, |d| d.expires - now < movement.shaman.directive_refresh_margin)
    {
        issue(commands);
    }
}

/// FREE formation point: centroid of living non-pet ENGAGED allies (excluding
/// self), offset behind the line and biased toward arena center, clamped into
/// Lightning-Bolt (wand) range of the kill target and into arena bounds.
/// `None` when no living non-pet ally exists (degenerate case).
fn compute_formation_point(
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    movement: &MovementConfig,
) -> Option<Vec3> {
    let shared = &movement.shared;
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

    // Wand-range pull: clamp the point into Lightning-Bolt range of the kill
    // target when it would sit outside it (weight 0 disables).
    if movement.shaman.weights.wand_pull > 0.0 {
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
