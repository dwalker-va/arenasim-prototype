//! Hunter AI — Ranged physical DPS with pet, traps, and dead zone management.
//!
//! The Hunter prioritizes maintaining distance and controlling space.
//! Key mechanics: dead zone (can't use ranged abilities within 8 yards),
//! kiting, trap placement, and pet coordination.
//!
//! ## Range Zone Priorities
//! - **Dead zone (<8 yards)**: Disengage > Frost Trap at feet > Kite
//! - **Closing (8-20 yards)**: Concussive Shot > Frost Trap > Kite + Arcane Shot
//! - **Safe (20-40 yards)**: Concussive Shot > Serpent Sting > Freezing Trap > Aimed Shot > Arcane Shot
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::constants::*;
use crate::states::play_match::decision_trace::{
    ActorView, DecisionEventBuilder, DecisionTrace, NoActionReason, RejectionReason, TargetView,
};
use super::{CombatContext, CombatantInfo};
use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};
use super::hunter_dip::{emit_dip_complete, HunterDipPlan};
use super::super::utils::log_ability_use;

/// Hold Concussive Shot while the target's existing slow has more than this many
/// seconds left; refresh only inside this window before expiry so the new slow
/// (after ~0.9s projectile travel) lands with no uptime gap and no wasted GCD.
const CONCUSSIVE_REFRESH_WINDOW: f32 = 1.0;

/// Hunter AI: Decides and executes abilities for a Hunter combatant.
pub fn decide_hunter_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    instant_attacks: &mut Vec<super::QueuedInstantAttack>,
    dip_plan: HunterDipPlan,
    decision_trace: &mut DecisionTrace,
) -> bool {
    let (nearest_enemy, nearest_distance) = find_nearest_enemy(entity, combatant.team, my_pos, ctx);

    let nearest_dist = nearest_distance.unwrap_or(40.0);

    // U4: Hunter-side pet dispatch. Runs independent of Hunter's own GCD —
    // pets have their own GCD pool, and Master's Call can self-cleanse even
    // when Hunter has no enemy target. Emits its own pet_decision trace
    // events (with `dispatched_by: Some(hunter_entity)`); the autonomous
    // headline-ability path in pet_ai.rs has been removed for Spider/Boar/Bird.
    dispatch_pet_ability(
        commands, abilities, decision_trace,
        entity, combatant.target, auras, ctx,
    );

    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let Some(target_entity) = combatant.target else {
        return false;
    };
    let Some(target_info) = ctx.combatants.get(&target_entity) else {
        return false;
    };
    if !target_info.is_alive {
        return false;
    }

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, Some(target_entity), my_pos) else {
        return false;
    };

    if ctx.entity_is_immune(target_entity) {
        builder.finish_no_action(NoActionReason::TargetImmune);
        return false;
    }

    // Freezing Trap DIP arrival (highest priority): the committed walk reached
    // throw range of the off-target enemy healer — drop the trap ON the healer's
    // position (not the midpoint) so the healer, not a chaser, triggers it. On
    // success install the dip-cleared posture and emit DipComplete; if the trap
    // can't land (arena clamp / lost the window) fall through and the dip
    // retries next tick.
    if let HunterDipPlan::DipCast { target, completed_state } = dip_plan {
        if ctx.combatants.get(&target).is_some_and(|i| i.is_alive) {
            // Lead a moving target into its path; drop directly on a planted one.
            let landing = super::hunter_dip::trap_lead_landing(ctx, target, my_pos)
                .unwrap_or(my_pos);
            if try_place_trap_at(
                commands, combat_log, abilities, entity, combatant, my_pos,
                landing, TrapType::Freezing, &mut builder,
            ) {
                builder.finish();
                commands.entity(entity).try_insert(completed_state);
                emit_dip_complete(decision_trace, ctx, target, my_pos);
                return true;
            }
        }
    }

    let distance_to_target = my_pos.distance(target_info.position);

    // === DEAD ZONE (<8 yards) — Escape priority ===
    if nearest_dist < HUNTER_DEAD_ZONE {
        let is_rooted = auras.map_or(false, |a| {
            a.auras.iter().any(|aura| aura.effect_type == AuraType::Root)
        });

        // Priority 1: Disengage (inline because it doesn't use a try_* helper)
        if let Some(def) = abilities.get(&AbilityType::Disengage) {
            let disengage = AbilityType::Disengage;
            if is_rooted {
                builder.reject(disengage, RejectionReason::Rooted);
            } else if let Some(remaining) = combatant.ability_cooldowns.get(&disengage) {
                builder.reject(disengage, RejectionReason::OnCooldown { remaining: *remaining });
            } else if combatant.current_mana < def.mana_cost {
                builder.reject(
                    disengage,
                    RejectionReason::InsufficientMana {
                        have: combatant.current_mana,
                        need: def.mana_cost,
                    },
                );
            } else {
                builder.choose(disengage, None, true);

                let away_dir = if let Some((enemy_entity, _)) = nearest_enemy {
                    if let Some(enemy_info) = ctx.combatants.get(&enemy_entity) {
                        (my_pos - enemy_info.position).normalize_or_zero()
                    } else {
                        Vec3::ZERO
                    }
                } else {
                    Vec3::ZERO
                };

                let direction = if away_dir == Vec3::ZERO {
                    if combatant.team == 1 { Vec3::new(-1.0, 0.0, 0.0) } else { Vec3::new(1.0, 0.0, 0.0) }
                } else {
                    Vec3::new(away_dir.x, 0.0, away_dir.z).normalize_or_zero()
                };

                commands.entity(entity).try_insert(DisengagingState {
                    direction,
                    distance_remaining: DISENGAGE_DISTANCE,
                });

                combatant.current_mana -= def.mana_cost;
                combatant.ability_cooldowns.insert(disengage, def.cooldown);
                combatant.global_cooldown = GCD;

                log_ability_use(combat_log, combatant.team, combatant.class, "Disengage", None, "uses");
                builder.finish();
                return true;
            }
        }

        // Priority 2: Frost Trap at feet
        if try_place_trap_at(
            commands, combat_log, abilities, entity, combatant, my_pos, my_pos,
            TrapType::Frost, &mut builder,
        ) {
            builder.finish();
            return true;
        }

        // Priority 3: no ability this tick — the ENGAGE/KITE posture machine
        // owns the flee movement now (proximity-gated KITE).
        builder.finish();
        return false;
    }

    // === CLOSING RANGE (8-20 yards) — Kite + instants ===
    if nearest_dist < 20.0 {
        let conc_max = abilities.get(&AbilityType::ConcussiveShot).map_or(35.0, |d| d.range);
        if let Some(ct) = concussive_target(ctx, entity, my_pos, target_entity, HUNTER_DEAD_ZONE, conc_max) {
            if try_concussive_shot(
                commands, combat_log, abilities, entity, combatant, my_pos,
                ct, ctx, &mut builder,
            ) {
                builder.finish();
                return true;
            }
        }

        // Frost Trap as a peel: aim the slow zone at the nearest kite-threat
        // MELEE (Warrior/Rogue) when one is present — that's the threat worth
        // slowing — rather than the nearest enemy generally (which can be a pet
        // or a stray-closest caster). Falls back to the nearest enemy when no
        // melee threat exists.
        let frost_anchor = super::dps_postures::nearest_melee_threat(ctx, entity, my_pos)
            .map(|(_, pos)| pos)
            .or_else(|| nearest_enemy.and_then(|(e, _)| ctx.combatants.get(&e).map(|i| i.position)));
        if let Some(anchor_pos) = frost_anchor {
            let midpoint = (my_pos + anchor_pos) / 2.0;
            if try_place_trap_at(
                commands, combat_log, abilities, entity, combatant, my_pos, midpoint,
                TrapType::Frost, &mut builder,
            ) {
                builder.finish();
                return true;
            }
        }

        // No Serpent Sting in the closing block: with melee bearing down,
        // every GCD belongs to the kiting toolkit. The sting applies from the
        // safe-range rotation and keeps ticking while we kite through this
        // band — that's the point of a DoT. (Sweep data: a sting GCD spent
        // during the approach window cost more than 50 DoT damage bought.)
        if try_arcane_shot(
            commands, combat_log, game_rng, abilities, entity, combatant, my_pos,
            target_entity, target_info, ctx, instant_attacks, auras, &mut builder,
        ) {
            builder.finish();
            return true;
        }

        // No ability this tick — KITE movement handles the kite.
        builder.finish();
        return false;
    }

    // === SAFE RANGE (20+ yards) — Full rotation ===

    // Burst-during-CC: when the enemy healer is hard-CC'd (e.g. our off-target
    // Freezing Trap just incap'd it) it can't heal the kill target — land the
    // Aimed Shot burst INSIDE that window instead of dribbling instants. Narrow
    // priority PREPEND, gated on the healer being CC'd AND the kill target being
    // someone other than that healer (bursting the healer would break its own
    // breakable trap). Outside the window the rotation below is the unchanged
    // order, so non-CC frames are untouched. This is the complement to the trap
    // rework: the trap creates the healer-down window, this converts it.
    let burst_window =
        ctx.enemy_healer_is_cced() && ctx.enemy_healer() != Some(target_entity);
    if burst_window && distance_to_target >= 20.0 {
        if try_aimed_shot(
            commands, combat_log, abilities, entity, combatant, my_pos,
            target_entity, target_info, auras, ctx, &mut builder,
        ) {
            builder.finish();
            return true;
        }
    }

    let conc_max = abilities.get(&AbilityType::ConcussiveShot).map_or(35.0, |d| d.range);
    if let Some(ct) = concussive_target(ctx, entity, my_pos, target_entity, HUNTER_DEAD_ZONE, conc_max) {
        if try_concussive_shot(
            commands, combat_log, abilities, entity, combatant, my_pos,
            ct, ctx, &mut builder,
        ) {
            builder.finish();
            return true;
        }
    }

    // Sting sits ABOVE Freezing Trap so the trap guard below always sees an
    // already-applied sting at placement time (closes the trap-placed-first,
    // stung-second ordering hole for the single-target case — plan KTD 3).
    if try_serpent_sting(
        commands, combat_log, abilities, entity, combatant, my_pos,
        target_entity, target_info, ctx, auras, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    // Freezing Trap. Preferred use is CC on the OFF-target enemy healer (the
    // one the team is NOT killing) — but a placed trap triggers on the first
    // enemy in radius, so we aim at the healer's POSITION (not the midpoint) and
    // HOLD unless the healer itself will trigger it: it must be in throw range
    // and no other enemy may be within the trigger radius of the landing. When
    // it can't land cleanly, we hold the trap for the dip rather than feed it to
    // the chaser. With no off-target healer (e.g. 1v1, or the healer IS the kill
    // target) we fall back to the legacy peel: trap the candidate at the
    // midpoint so the Hunter keeps its melee-peel in healer-less matchups.
    let off_target = super::hunter_dip::opportunistic_off_target(
        ctx, entity, combatant.team, combatant.target, my_pos,
    );
    if let Some((healer, healer_pos)) = off_target {
        // Only drop opportunistically when already point-blank — the dip walks us
        // in when we're farther out. Lead the landing into the target's path
        // (planted target → directly on it) so the 1.5s arm doesn't whiff a kite.
        let plant_close = my_pos.distance(healer_pos) <= super::hunter_dip::HUNTER_TRAP_PLANT_RANGE;
        let landing = super::hunter_dip::trap_lead_landing(ctx, healer, my_pos).unwrap_or(healer_pos);
        // No other living enemy close enough to the landing to beat the target
        // to the trigger.
        let healer_triggers = !ctx.combatants.values().any(|other| {
            other.team != combatant.team
                && other.is_alive
                && other.entity != healer
                && other.position.distance(landing) <= TRAP_TRIGGER_RADIUS
        });
        if plant_close && healer_triggers {
            // Two-way CC guard (R8/R9): a friendly DoT on the healer pops the
            // incap on the first tick — skip (only when the trap is castable).
            if ctx.has_friendly_dots_on_target(healer)
                && !combatant.ability_cooldowns.contains_key(&AbilityType::FreezingTrap)
            {
                builder.reject(AbilityType::FreezingTrap, RejectionReason::FriendlyBreakableCC);
            } else if try_place_trap_at(
                commands, combat_log, abilities, entity, combatant, my_pos,
                landing, TrapType::Freezing, &mut builder,
            ) {
                builder.finish();
                return true;
            }
        }
        // else: HOLD — the dip will walk us into range to land it on the healer.
    } else {
        let trap_target = freezing_trap_candidate(ctx, target_entity);
        if let Some(trap_target_info) = ctx.combatants.get(&trap_target).filter(|info| info.is_alive) {
            // Two-way CC guard (R8/R9): never aim Freezing Trap at a target the
            // team has DoT'd — the first tick breaks the incapacitate
            // (break_on_damage: 0.0). Reactive and binary: skip this tick, no
            // fallthrough to a second candidate. Only traced when the trap is
            // otherwise castable — while it's on cooldown, fall through so the
            // trace records OnCooldown instead of masking it as the DoT guard.
            if ctx.has_friendly_dots_on_target(trap_target)
                && !combatant.ability_cooldowns.contains_key(&AbilityType::FreezingTrap)
            {
                builder.reject(AbilityType::FreezingTrap, RejectionReason::FriendlyBreakableCC);
            } else if try_place_trap_at(
                commands, combat_log, abilities, entity, combatant, my_pos,
                (my_pos + trap_target_info.position) / 2.0,
                TrapType::Freezing, &mut builder,
            ) {
                builder.finish();
                return true;
            }
        }
    }

    if distance_to_target >= 20.0 {
        if try_aimed_shot(
            commands, combat_log, abilities, entity, combatant, my_pos,
            target_entity, target_info, auras, ctx, &mut builder,
        ) {
            builder.finish();
            return true;
        }
    } else {
        builder.reject(
            AbilityType::AimedShot,
            RejectionReason::WithinDeadZone {
                distance: distance_to_target,
                min: 20.0,
            },
        );
    }

    if try_arcane_shot(
        commands, combat_log, game_rng, abilities, entity, combatant, my_pos,
        target_entity, target_info, ctx, instant_attacks, auras, &mut builder,
    ) {
        builder.finish();
        return true;
    }

    builder.finish();
    false
}

// ==============================================================================
// Helper Functions
// ==============================================================================

/// The entity Freezing Trap wants: the enemy healer, falling back to the kill
/// target. Single source of truth shared by trap placement and the sting's
/// trap-candidate reservation — if these drifted apart, the sting would
/// silently re-open the trap-suppression hole the reservation closes.
fn freezing_trap_candidate(ctx: &CombatContext, fallback: Entity) -> Entity {
    ctx.enemy_healer().unwrap_or(fallback)
}

fn find_nearest_enemy(self_entity: Entity, my_team: u8, my_pos: Vec3, ctx: &CombatContext) -> (Option<(Entity, f32)>, Option<f32>) {
    let mut nearest: Option<(Entity, f32)> = None;
    for (entity, info) in ctx.combatants.iter() {
        if *entity == self_entity || info.team == my_team || !info.is_alive || info.is_pet || info.stealthed {
            continue;
        }
        let dist = my_pos.distance(info.position);
        if nearest.map_or(true, |(_, d)| dist < d) {
            nearest = Some((*entity, dist));
        }
    }
    let distance = nearest.map(|(_, d)| d);
    (nearest, distance)
}

/// Remaining duration of the longest active `MovementSpeedSlow` on `target`,
/// or `None` if it isn't slowed. (`Aura.duration` ticks down to zero, so it is
/// the time remaining.)
fn slow_remaining(target: Entity, ctx: &CombatContext) -> Option<f32> {
    ctx.active_auras.get(&target).and_then(|auras| {
        auras
            .iter()
            .filter(|a| a.effect_type == AuraType::MovementSpeedSlow)
            .map(|a| a.duration)
            .fold(None, |acc: Option<f32>, d| Some(acc.map_or(d, |m| m.max(d))))
    })
}

/// The Hunter's preferred Concussive Shot target, encoding the slow heuristics:
///  1. **Peel the nearest melee kite-threat** (Warrior/Rogue) — a slow on a melee
///     is almost always worth it — even when it isn't the kill target.
///  2. **Otherwise the kill target, but only while it is actually moving** — a
///     slow is wasted on a stationary/casting caster (uses the snapshot
///     `velocity`, which is zero while casting/channeling).
/// A candidate is skipped if it is out of range, already slowed with more than
/// `CONCUSSIVE_REFRESH_WINDOW` left (hold; refresh only just before it expires so
/// there's no uptime gap), or under a friendly break-on-damage CC (a no-op here
/// since Concussive deals no damage, but kept as a guard).
fn concussive_target(
    ctx: &CombatContext,
    entity: Entity,
    my_pos: Vec3,
    kill_target: Entity,
    min_range: f32,
    max_range: f32,
) -> Option<Entity> {
    let castable = |e: Entity| {
        ctx.combatants.get(&e).map_or(false, |i| {
            let d = my_pos.distance(i.position);
            i.is_alive
                && d >= min_range
                && d <= max_range
                && !ctx.has_friendly_breakable_cc(e)
                && slow_remaining(e, ctx).map_or(true, |r| r <= CONCUSSIVE_REFRESH_WINDOW)
        })
    };
    // 1. Nearest melee kite-threat — peel it, moving or not.
    if let Some((m, _)) = super::dps_postures::nearest_melee_threat(ctx, entity, my_pos) {
        if castable(m) {
            return Some(m);
        }
    }
    // 2. Else the kill target, but only while it is actually moving.
    if castable(kill_target)
        && ctx
            .combatants
            .get(&kill_target)
            .map_or(false, |i| i.velocity.length() > 0.5)
    {
        return Some(kill_target);
    }
    None
}

/// Attempt to place a trap at a specific position (or at the Hunter's feet).
fn try_place_trap_at(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    position: Vec3,
    trap_type: TrapType,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = match trap_type {
        TrapType::Freezing => AbilityType::FreezingTrap,
        TrapType::Frost => AbilityType::FrostTrap,
    };

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

    // Clamp to octagonal arena bounds (midpoint can land outside corners)
    let position = crate::states::play_match::combat_core::clamp_to_arena(position);

    let trap_name = trap_type.name();

    let distance = Vec3::new(my_pos.x, 0.0, my_pos.z)
        .distance(Vec3::new(position.x, 0.0, position.z));

    if distance > TRAP_LAUNCH_MIN_RANGE {
        let origin = Vec3::new(my_pos.x, 1.5, my_pos.z);
        let landing = Vec3::new(position.x, 0.0, position.z);
        let direction = (landing - origin).normalize_or_zero();
        let rotation = if direction != Vec3::ZERO {
            Quat::from_rotation_y(direction.x.atan2(direction.z))
        } else {
            Quat::IDENTITY
        };
        commands.spawn((
            Transform::from_translation(origin).with_rotation(rotation),
            TrapLaunchProjectile {
                trap_type,
                owner_team: combatant.team,
                owner: entity,
                origin,
                landing_position: landing,
                total_distance: distance,
                distance_traveled: 0.0,
            },
            PlayMatchEntity,
        ));
        log_ability_use(combat_log, combatant.team, combatant.class, trap_name, None, "uses");
    } else {
        commands.spawn((
            Transform::from_translation(Vec3::new(position.x, 0.0, position.z)),
            Trap {
                trap_type,
                owner_team: combatant.team,
                owner: entity,
                arm_timer: TRAP_ARM_DELAY,
                trigger_radius: TRAP_TRIGGER_RADIUS,
                triggered: false,
            },
            PlayMatchEntity,
        ));
        log_ability_use(combat_log, combatant.team, combatant.class, trap_name, None, "uses");
    }

    combatant.current_mana -= def.mana_cost;
    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;

    true
}

/// Try Concussive Shot — fires a projectile that slows on arrival.
fn try_concussive_shot(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::ConcussiveShot;
    let Some(def) = abilities.get(&ability) else { return false };

    let Some(target_info) = ctx.combatants.get(&target_entity) else { return false };
    let target_pos = target_info.position;

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        ability, def, combatant, my_pos, None,
        Some((target_entity, target_pos)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, None,
                Some((target_entity, target_pos)), ctx, opts,
            ),
        );
        return false;
    }

    // Hold if already slowed with real time left; allow a refresh just before
    // expiry (no uptime gap, no wasted GCD re-slowing).
    if slow_remaining(target_entity, ctx).map_or(false, |r| r > CONCUSSIVE_REFRESH_WINDOW) {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }

    builder.choose(ability, Some(target_entity), true);

    let projectile_speed = def.projectile_speed.unwrap_or(40.0);
    commands.spawn((
        Projectile {
            caster: entity,
            target: target_entity,
            ability,
            speed: projectile_speed,
            caster_team: combatant.team,
            caster_class: combatant.class,
        },
        Transform::from_translation(my_pos + Vec3::new(0.0, 1.5, 0.0)),
        PlayMatchEntity,
    ));

    combatant.current_mana -= def.mana_cost;
    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, Some((target_info.team, target_info.class)), "fires");

    true
}

/// Try Aimed Shot (2.5s cast time ranged ability).
fn try_aimed_shot(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_info: &super::CombatantInfo,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::AimedShot;
    let Some(def) = abilities.get(&ability) else { return false };

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        ability, def, combatant, my_pos, auras,
        Some((target_entity, target_info.position)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, auras,
                Some((target_entity, target_info.position)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), false);

    let cast_time = calculate_cast_time(def.cast_time, auras);
    commands.entity(entity).insert(CastingState::new(ability, target_entity, cast_time));

    combatant.current_mana -= def.mana_cost;
    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, Some((target_info.team, target_info.class)), "begins casting");

    true
}

/// Try Arcane Shot — fires a projectile (damage applied on arrival).
fn try_arcane_shot(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    _game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_info: &super::CombatantInfo,
    ctx: &CombatContext,
    _instant_attacks: &mut Vec<super::QueuedInstantAttack>,
    auras: Option<&ActiveAuras>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::ArcaneShot;
    let Some(def) = abilities.get(&ability) else { return false };

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        ability, def, combatant, my_pos, auras,
        Some((target_entity, target_info.position)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, auras,
                Some((target_entity, target_info.position)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), true);

    let projectile_speed = def.projectile_speed.unwrap_or(45.0);
    commands.spawn((
        Projectile {
            caster: entity,
            target: target_entity,
            ability,
            speed: projectile_speed,
            caster_team: combatant.team,
            caster_class: combatant.class,
        },
        Transform::from_translation(my_pos + Vec3::new(0.0, 1.5, 0.0)),
        PlayMatchEntity,
    ));

    combatant.current_mana -= def.mana_cost;
    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, Some((target_info.team, target_info.class)), "fires");

    true
}

/// Serpent Sting: instant, no-cooldown Nature DoT projectile — the Hunter's
/// kiting damage. Pure DoT (zero direct damage), so the projectile applies the
/// aura via the non-damage branch and its impact can never break CC.
///
/// Dedup keys on `effect_type == DamageOverTime && ability_name == "Serpent
/// Sting"` (the Corruption idiom — never `AuraType` alone, so stings and
/// Warlock DoTs coexist). No cooldown is inserted: the ability has none, and
/// `pre_cast_ok`'s cooldown check passes on an absent key.
fn try_serpent_sting(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_info: &super::CombatantInfo,
    ctx: &CombatContext,
    auras: Option<&ActiveAuras>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::SerpentSting;
    let Some(def) = abilities.get(&ability) else { return false };

    let target_has_sting = ctx.active_auras
        .get(&target_entity)
        .map(|target_auras| target_auras.iter().any(|a|
            a.effect_type == AuraType::DamageOverTime && a.ability_name == "Serpent Sting"
        ))
        .unwrap_or(false);

    if target_has_sting {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }

    // Mana floor: the sting is a luxury good, the kiting toolkit (Concussive /
    // Frost Trap / Disengage) is survival. Sweep data showed the sting draining
    // the fixed 240 pool and starving mobility vs melee (1v1 vs Warrior
    // 100% -> 34% without this floor). The sting's own cost counts against the
    // floor so the reserve holds AFTER the cast, not just before it.
    const STING_MANA_FLOOR: f32 = 100.0;
    if combatant.current_mana - def.mana_cost < STING_MANA_FLOOR {
        builder.reject(ability, RejectionReason::PreconditionUnmet {
            note: "mana reserved for kiting toolkit".to_string(),
        });
        return false;
    }

    // Never sting a rage user: they convert 15% of damage taken into rage
    // (auto_attack.rs / auras.rs), so a permanent DoT is a steady rage faucet
    // funding extra Mortal Strikes against our team. Sweep data: stinging
    // Warriors was uniquely immune to every other mitigation.
    if target_info.class.gains_rage_from_damage() {
        builder.reject(ability, RejectionReason::PreconditionUnmet {
            note: "sting feeds Warrior rage".to_string(),
        });
        return false;
    }

    // Trap-candidate reservation: don't sting the Freezing Trap candidate while
    // the trap is poised to fire at it — a ticking sting would suppress the
    // trap permanently via the friendly-DoT guard (the kill target is often the
    // enemy healer, which is exactly who the trap wants). Once the trap is on
    // cooldown the sting resumes freely. "Poised" requires the candidate to be
    // free of OTHER friendly DoTs too: if a teammate's Corruption already
    // blocks the trap, reserving the sting as well would deadlock both
    // abilities for the rest of the match. (Mana isn't checked — the sting
    // floor above already guarantees the trap's cost.)
    let trap_poised = !combatant.ability_cooldowns.contains_key(&AbilityType::FreezingTrap)
        && !ctx.has_friendly_dots_on_target(target_entity);
    if trap_poised && freezing_trap_candidate(ctx, target_entity) == target_entity {
        builder.reject(ability, RejectionReason::PreconditionUnmet {
            note: "trap candidate reserved for Freezing Trap".to_string(),
        });
        return false;
    }

    let opts = PreCastOpts { check_friendly_cc: true, ..Default::default() };
    if !pre_cast_ok(
        ability, def, combatant, my_pos, auras,
        Some((target_entity, target_info.position)), ctx, opts,
    ) {
        builder.reject(
            ability,
            classify_pre_cast_failure(
                ability, def, combatant, my_pos, auras,
                Some((target_entity, target_info.position)), ctx, opts,
            ),
        );
        return false;
    }

    builder.choose(ability, Some(target_entity), true);

    let projectile_speed = def
        .projectile_speed
        .expect("Serpent Sting projectile_speed is contract-tested (ability_tests.rs)");
    commands.spawn((
        Projectile {
            caster: entity,
            target: target_entity,
            ability,
            speed: projectile_speed,
            caster_team: combatant.team,
            caster_class: combatant.class,
        },
        Transform::from_translation(my_pos + Vec3::new(0.0, 1.5, 0.0)),
        PlayMatchEntity,
    ));

    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    log_ability_use(combat_log, combatant.team, combatant.class, &def.name, Some((target_info.team, target_info.class)), "fires");

    true
}

// ==============================================================================
// Pet headline ability dispatch (U4 — hybrid model)
// ==============================================================================
//
// Hunter AI owns the dispatch decision for headline pet abilities (Spider Web,
// Boar Charge, Master's Call). Snapshot heuristics gate dispatch here; pet AI
// runs authoritative checks at execution time when consuming the PetCommand
// (see `class_ai/pet_ai.rs::pet_command_rejection`). Hunter dispatch is
// non-exclusive with Hunter's own ability cast — different GCD pools — so a
// single tick can fire both.

/// Route pet-dispatch helpers based on the pet's PetType. Hunter has exactly
/// one pet (Spider, Boar, or Bird) per match; Felhunter belongs to Warlock and
/// is not Hunter-dispatched.
fn dispatch_pet_ability(
    commands: &mut Commands,
    abilities: &AbilityDefinitions,
    decision_trace: &mut DecisionTrace,
    hunter_entity: Entity,
    target_entity: Option<Entity>,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) {
    let Some(hunter_info) = ctx.combatants.get(&hunter_entity) else { return };
    let Some(pet_entity) = hunter_info.pet else { return };
    let Some(pet_info) = ctx.combatants.get(&pet_entity) else { return };
    let Some(pet_type) = pet_info.pet_type else { return };

    match pet_type {
        PetType::Spider => {
            if let Some(target) = target_entity {
                try_dispatch_spider_web(
                    commands, abilities, decision_trace,
                    hunter_entity, pet_entity, pet_info, target, ctx,
                );
            }
        }
        PetType::Boar => {
            if let Some(target) = target_entity {
                try_dispatch_boar_charge(
                    commands, abilities, decision_trace,
                    hunter_entity, pet_entity, pet_info, target, ctx,
                );
            }
        }
        PetType::Bird => {
            try_dispatch_masters_call(
                commands, abilities, decision_trace,
                hunter_entity, pet_entity, pet_info, hunter_info, auras, ctx,
            );
        }
        PetType::Felhunter => {
            // Not Hunter-dispatched (belongs to Warlock).
        }
    }
}

/// Try to dispatch Spider Web onto the Hunter's current target. Snapshot
/// heuristics only — pet AI re-validates at execution time. Emits a
/// pet_decision trace event with `dispatched_by: Some(hunter_entity)`.
fn try_dispatch_spider_web(
    commands: &mut Commands,
    abilities: &AbilityDefinitions,
    decision_trace: &mut DecisionTrace,
    hunter_entity: Entity,
    pet_entity: Entity,
    pet_info: &CombatantInfo,
    target_entity: Entity,
    ctx: &CombatContext,
) -> bool {
    let ability = AbilityType::SpiderWeb;
    let Some(def) = abilities.get(&ability) else { return false };
    let Some(target_info) = ctx.combatants.get(&target_entity) else { return false };

    let pet_pos = pet_info.position;
    let actor_view = ActorView::from_info(pet_info);
    let target_view = TargetView::from_info(target_info, pet_pos);
    let mut builder = decision_trace.start_pet_dispatch_decision(
        actor_view,
        Some(target_view),
        hunter_entity,
        pet_info.pet_type.map_or("Spider", |pt| pt.name()),
        hunter_entity,
    );

    if let Some(reason) = dispatch_predicates_for_damaging(ability, def, pet_info, pet_entity, target_entity, target_info, ctx) {
        builder.reject(ability, reason);
        builder.finish();
        return false;
    }

    // Avoid re-rooting an already-rooted target (Spider Web stacks the aura
    // but the visible behavior is identical — keep the trace honest with
    // `AlreadyApplied` so audits don't double-count effective dispatches).
    if let Some(auras) = ctx.active_auras.get(&target_entity) {
        if auras.iter().any(|a| a.effect_type == AuraType::Root) {
            builder.reject(ability, RejectionReason::AlreadyApplied);
            builder.finish();
            return false;
        }
    }

    builder.choose(ability, Some(target_entity), true);
    builder.finish();

    commands.entity(pet_entity).try_insert(PetCommand {
        ability,
        target: target_entity,
        dispatched_by: hunter_entity,
    });
    true
}

/// Try to dispatch Boar Charge onto the Hunter's current target.
fn try_dispatch_boar_charge(
    commands: &mut Commands,
    abilities: &AbilityDefinitions,
    decision_trace: &mut DecisionTrace,
    hunter_entity: Entity,
    pet_entity: Entity,
    pet_info: &CombatantInfo,
    target_entity: Entity,
    ctx: &CombatContext,
) -> bool {
    let ability = AbilityType::BoarCharge;
    let Some(def) = abilities.get(&ability) else { return false };
    let Some(target_info) = ctx.combatants.get(&target_entity) else { return false };

    let pet_pos = pet_info.position;
    let actor_view = ActorView::from_info(pet_info);
    let target_view = TargetView::from_info(target_info, pet_pos);
    let mut builder = decision_trace.start_pet_dispatch_decision(
        actor_view,
        Some(target_view),
        hunter_entity,
        pet_info.pet_type.map_or("Boar", |pt| pt.name()),
        hunter_entity,
    );

    if let Some(reason) = dispatch_predicates_for_damaging(ability, def, pet_info, pet_entity, target_entity, target_info, ctx) {
        builder.reject(ability, reason);
        builder.finish();
        return false;
    }

    builder.choose(ability, Some(target_entity), true);
    builder.finish();

    commands.entity(pet_entity).try_insert(PetCommand {
        ability,
        target: target_entity,
        dispatched_by: hunter_entity,
    });
    true
}

/// Try to dispatch Master's Call. Prefers Hunter (self) if owner is rooted or
/// slowed; falls back to scanning allies on the Hunter's team.
fn try_dispatch_masters_call(
    commands: &mut Commands,
    abilities: &AbilityDefinitions,
    decision_trace: &mut DecisionTrace,
    hunter_entity: Entity,
    pet_entity: Entity,
    pet_info: &CombatantInfo,
    hunter_info: &CombatantInfo,
    hunter_auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) -> bool {
    let ability = AbilityType::MastersCall;
    let Some(def) = abilities.get(&ability) else { return false };

    // Find a cleanse target: Hunter first (uses live auras since that's the
    // freshest view); then scan allies in the snapshot.
    let owner_needs_cleanse = hunter_auras.map_or(false, |a| {
        a.auras.iter().any(|aura| matches!(
            aura.effect_type,
            AuraType::Root | AuraType::MovementSpeedSlow,
        ))
    });
    let cleanse_target = if owner_needs_cleanse {
        Some(hunter_entity)
    } else {
        let mut fallback: Option<Entity> = None;
        for (ally_entity, info) in ctx.combatants.iter() {
            if info.team != hunter_info.team || !info.is_alive || info.is_pet {
                continue;
            }
            if let Some(auras) = ctx.active_auras.get(ally_entity) {
                if auras.iter().any(|a| matches!(
                    a.effect_type,
                    AuraType::Root | AuraType::MovementSpeedSlow,
                )) {
                    fallback = Some(*ally_entity);
                    break;
                }
            }
        }
        fallback
    };

    let pet_pos = pet_info.position;
    let actor_view = ActorView::from_info(pet_info);
    let target_view = cleanse_target
        .and_then(|t| ctx.combatants.get(&t))
        .map(|info| TargetView::from_info(info, pet_pos));
    let mut builder = decision_trace.start_pet_dispatch_decision(
        actor_view,
        target_view,
        hunter_entity,
        pet_info.pet_type.map_or("Bird", |pt| pt.name()),
        hunter_entity,
    );

    if pet_info.health_pct() < 0.25 {
        builder.reject(ability, RejectionReason::LowHealthHeel);
        builder.finish();
        return false;
    }

    let cd_remaining = ctx.ability_cooldowns
        .get(&pet_entity)
        .and_then(|cds| cds.get(&ability))
        .copied()
        .unwrap_or(0.0);
    if cd_remaining > 0.0 {
        builder.reject(ability, RejectionReason::OnCooldown { remaining: cd_remaining });
        builder.finish();
        return false;
    }

    let Some(target) = cleanse_target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        builder.finish();
        return false;
    };

    if let Some(target_info) = ctx.combatants.get(&target) {
        let dist = pet_pos.distance(target_info.position);
        if dist > def.range {
            builder.reject(ability, RejectionReason::OutOfRange { distance: dist, max: def.range });
            builder.finish();
            return false;
        }
    }

    builder.choose(ability, Some(target), true);
    builder.finish();

    commands.entity(pet_entity).try_insert(PetCommand {
        ability,
        target,
        dispatched_by: hunter_entity,
    });
    true
}

/// Snapshot-side predicate check for damaging/charge pet abilities (Spider
/// Web, Boar Charge). Returns the first failing rejection reason, or `None`
/// if all snapshot heuristics pass.
fn dispatch_predicates_for_damaging(
    ability: AbilityType,
    def: &crate::states::play_match::ability_config::AbilityConfig,
    pet_info: &CombatantInfo,
    pet_entity: Entity,
    target_entity: Entity,
    target_info: &CombatantInfo,
    ctx: &CombatContext,
) -> Option<RejectionReason> {
    if pet_info.health_pct() < 0.25 {
        return Some(RejectionReason::LowHealthHeel);
    }

    let cd_remaining = ctx.ability_cooldowns
        .get(&pet_entity)
        .and_then(|cds| cds.get(&ability))
        .copied()
        .unwrap_or(0.0);
    if cd_remaining > 0.0 {
        return Some(RejectionReason::OnCooldown { remaining: cd_remaining });
    }

    if !target_info.is_alive {
        return Some(RejectionReason::NoValidTarget);
    }
    if target_info.is_pet || target_info.team == pet_info.team {
        return Some(RejectionReason::NoValidTarget);
    }
    if target_info.stealthed {
        return Some(RejectionReason::NoValidTarget);
    }

    let dist = pet_info.position.distance(target_info.position);
    if dist > def.range {
        return Some(RejectionReason::OutOfRange { distance: dist, max: def.range });
    }
    if ability == AbilityType::BoarCharge && dist < CHARGE_MIN_RANGE {
        return Some(RejectionReason::WithinDeadZone { distance: dist, min: CHARGE_MIN_RANGE });
    }

    // Friendly-CC guard only applies to abilities that deal damage on landing
    // — Spider Web is a 0-damage Root aura and cannot break a friendly CC by
    // existing on the target. Boar Charge does deal impact damage and would
    // break a threshold-0 friendly CC (Polymorph, Freezing Trap incap).
    if ability == AbilityType::BoarCharge && ctx.has_friendly_breakable_cc(target_entity) {
        return Some(RejectionReason::FriendlyBreakableCC);
    }

    None
}
