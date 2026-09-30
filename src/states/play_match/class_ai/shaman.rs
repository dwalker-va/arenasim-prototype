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
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::*;
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, RejectionReason,
};
use crate::states::play_match::movement_config::MovementConfig;

use super::super::utils::{combatant_id, log_ability_use};
use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};
use super::caster_healer_posture::CasterHealerPlan;
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
    plan: &CasterHealerPlan,
    movement: &MovementConfig,
    decision_trace: &mut DecisionTrace,
) -> bool {
    let escape_defer = plan.escape_defer;

    // GCD gate — at most one ability per GCD.
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, combatant.target, my_pos)
    else {
        return false;
    };

    // P1: emergency heal — a dying ally is topped off before ANYTHING else
    // (including totem maintenance: never let an ally die to refresh a buff
    // totem). Critical: never deferred for an escape window.
    if try_lesser_healing_wave(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        SHAMAN_EMERGENCY_HP,
        None,
        movement,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // P2: Frost Shock — instant Frost nuke + slow, used as a peel against a
    // melee/pet attacking the Shaman or a low-HP ally.
    if try_frost_shock(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // P3: keep the four element totems up (consumes the GCD when one drops).
    // Refreshes are deferred below a mana floor so totems don't starve offense;
    // initial drops are always allowed.
    if maintain_totems(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        totem_durations,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // P4: Lightning Bolt — cast-time filler nuke on the kill target. Deferred
    // while fleeing (don't hardcast mid-escape).
    if try_lightning_bolt(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        escape_defer,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // P5: sustain heal — top off an injured (non-emergency) ally. Lighter gate
    // than the Priest: the Shaman is offense-slanted, so it only heals when
    // needed. Deferred for healthy-ish targets while an escape window is live.
    if try_lesser_healing_wave(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        SHAMAN_SUSTAIN_HP,
        escape_defer,
        movement,
        &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // P6: Purge — strip a beneficial aura off an enemy (prefers the healer).
    if super::try_purge_enemy(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        auras,
        ctx,
        &mut builder,
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
    if !pre_cast_ok(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((heal_target, target_pos)),
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
                Some((heal_target, target_pos)),
                ctx,
                opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(heal_target), false);

    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras);
    commands
        .entity(entity)
        .insert(CastingState::new(ability, heal_target, cast_time));

    let target_tuple = ctx.combatants.get(&heal_target).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        target_tuple,
        "begins casting",
    );

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
                    || info.target.is_some_and(|t| low_allies.contains(&t)))
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
        ctx.combatants.get(t).is_some_and(|i| {
            i.is_alive && my_pos.distance(i.position) <= range && !ctx.entity_is_immune(*t)
        })
    })
}

/// Try to cast Frost Shock — instant Frost nuke that applies a non-breaking
/// slow (a peel). Routed through `CastingState` with the ability's 0.0 cast
/// time so the generic completion path applies BOTH the damage (correct
/// school + spell-power scaling) and the slow aura (`def.applies_aura`) — the
/// same generic path Lightning Bolt and Lesser Healing Wave use. The 6s cooldown
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

    builder.choose(ability, Some(target_entity), true);

    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;
    let cast_time = calculate_cast_time(def.cast_time, auras); // 0.0 — completes immediately
    commands
        .entity(entity)
        .insert(CastingState::new(ability, target_entity, cast_time));

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        target_tuple,
        "casts",
    );

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

    let target_tuple = ctx.combatants.get(&target_entity).map(|info| info.log_id());
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        target_tuple,
        "begins casting",
    );

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
            TotemElement::Air => try_air_totem(
                commands, combat_log, abilities, entity, combatant, my_pos, builder,
            ),
            TotemElement::Water => try_water_totem(
                commands, combat_log, abilities, entity, combatant, my_pos, builder,
            ),
            TotemElement::Earth => try_earth_totem(
                commands, combat_log, abilities, entity, combatant, my_pos, builder,
            ),
            TotemElement::Fire => try_fire_totem(
                commands, combat_log, abilities, entity, combatant, my_pos, builder,
            ),
        };
        if cast {
            return true;
        }
    }
    false
}

/// Per-element ability/buff mapping. MODEST magnitudes — real balance is a later
/// unit. Mirrors the `TotemElement -> buff` table documented in the U3 spec.
/// The `(buff aura, magnitude)` a totem ability grants, sourced from
/// [`totem_spec`] so UI/tooltips read the SAME number gameplay uses — they can
/// never drift. Returns `None` for non-totem abilities.
pub fn totem_buff_spec(ability: AbilityType) -> Option<(AuraType, f32)> {
    let element = match ability {
        AbilityType::AirTotem => TotemElement::Air,
        AbilityType::WaterTotem => TotemElement::Water,
        AbilityType::EarthTotem => TotemElement::Earth,
        AbilityType::FireTotem => TotemElement::Fire,
        _ => return None,
    };
    let (_, aura_type, magnitude, _) = totem_spec(element);
    Some((aura_type, magnitude))
}

/// Per-element totem spec. `pub` so the Animation Sandbox can spawn a faithful
/// totem, the encyclopedia can catalog the pulsed buff, and
/// `tests/aura_catalog_audit.rs` can check the catalog against it — all from the
/// SAME data gameplay uses (single source of truth — none of them can drift
/// from the real totem). Pure; no behavior change.
pub fn totem_spec(element: TotemElement) -> (AbilityType, AuraType, f32, SpellSchool) {
    match element {
        // Windfury Totem — empowers melee allies' auto-attacks (proc chance 0..1).
        TotemElement::Air => (
            AbilityType::AirTotem,
            AuraType::WindfuryBuff,
            0.12,
            SpellSchool::Nature,
        ),
        // Healing Stream Totem — periodic ally heal (per-tick amount).
        TotemElement::Water => (
            AbilityType::WaterTotem,
            AuraType::HealingOverTime,
            8.0,
            SpellSchool::Nature,
        ),
        // Strength of Earth Totem — flat attack power. Tempered alongside
        // Flametongue (SP) so physical partners (Warrior/Rogue/Hunter) don't
        // win the damage race against a Priest-healed mirror by totem buffs alone.
        TotemElement::Earth => (
            AbilityType::EarthTotem,
            AuraType::AttackPowerIncrease,
            15.0,
            SpellSchool::Nature,
        ),
        // Flametongue Totem — flat spell power.
        TotemElement::Fire => (
            AbilityType::FireTotem,
            AuraType::SpellPowerIncrease,
            18.0,
            SpellSchool::Fire,
        ),
    }
}

/// Deterministic per-element horizontal offset so the four totems fan out
/// around the Shaman's feet (compass directions at 0/90/180/270 degrees by
/// element index). No RNG — required for seeded-replay determinism.
pub(crate) fn totem_spacing_offset(element: TotemElement) -> Vec3 {
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

    let Some(def) = abilities.get(&ability) else {
        return false;
    };

    if let Some(remaining) = combatant.ability_cooldowns.get(&ability) {
        builder.reject(
            ability,
            RejectionReason::OnCooldown {
                remaining: *remaining,
            },
        );
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
        format!(
            "[TOTEM] {} drops {}",
            combatant_id(team, combatant.slot, combatant.class),
            element.buff_name()
        ),
    );
    log_ability_use(
        combat_log,
        team,
        combatant.slot,
        combatant.class,
        &def.name,
        None,
        "drops",
    );

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
    try_totem(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        TotemElement::Air,
        builder,
    )
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
    try_totem(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        TotemElement::Water,
        builder,
    )
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
    try_totem(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        TotemElement::Earth,
        builder,
    )
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
    try_totem(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        TotemElement::Fire,
        builder,
    )
}
