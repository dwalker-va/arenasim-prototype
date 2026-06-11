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
use crate::states::match_config::CharacterClass;
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
use super::super::utils::log_ability_use;

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
    decision_trace: &mut DecisionTrace,
) -> bool {
    let (nearest_enemy, nearest_distance) = find_nearest_enemy(entity, combatant.team, my_pos, ctx);

    let nearest_dist = nearest_distance.unwrap_or(40.0);
    let nearest_is_melee = nearest_enemy
        .and_then(|(e, _)| ctx.combatants.get(&e))
        .map_or(false, |info| info.class.is_melee());
    if nearest_is_melee && nearest_dist < HUNTER_KITE_RANGE {
        combatant.kiting_timer = combatant.kiting_timer.max(0.5);
    }

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

        // Priority 3: Set kiting timer to flee
        combatant.kiting_timer = 3.0;
        builder.finish();
        return false;
    }

    // === CLOSING RANGE (8-20 yards) — Kite + instants ===
    if nearest_dist < 20.0 {
        if let Some((enemy_entity, _)) = nearest_enemy {
            if try_concussive_shot(
                commands, combat_log, abilities, entity, combatant, my_pos,
                enemy_entity, ctx, &mut builder,
            ) {
                combatant.kiting_timer = 3.0;
                builder.finish();
                return true;
            }
        }

        if let Some((enemy_entity, _)) = nearest_enemy {
            if let Some(enemy_info) = ctx.combatants.get(&enemy_entity) {
                let midpoint = (my_pos + enemy_info.position) / 2.0;
                if try_place_trap_at(
                    commands, combat_log, abilities, entity, combatant, my_pos, midpoint,
                    TrapType::Frost, &mut builder,
                ) {
                    combatant.kiting_timer = 3.0;
                    builder.finish();
                    return true;
                }
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
            combatant.kiting_timer = 3.0;
            builder.finish();
            return true;
        }

        combatant.kiting_timer = 3.0;
        builder.finish();
        return false;
    }

    // === SAFE RANGE (20+ yards) — Full rotation ===

    if try_concussive_shot(
        commands, combat_log, abilities, entity, combatant, my_pos,
        target_entity, ctx, &mut builder,
    ) {
        builder.finish();
        return true;
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

    let freezing_trap_target = ctx.enemy_healer().or(Some(target_entity));
    if let Some(trap_target) = freezing_trap_target {
        if let Some(trap_target_info) = ctx.combatants.get(&trap_target) {
            if trap_target_info.is_alive {
                // Two-way CC guard (R8/R9): never aim Freezing Trap at a target
                // the team has DoT'd — the first tick breaks the incapacitate
                // (break_on_damage: 0.0). Reactive and binary: skip this tick,
                // no fallthrough to a second candidate.
                if ctx.has_friendly_dots_on_target(trap_target) {
                    builder.reject(AbilityType::FreezingTrap, RejectionReason::FriendlyBreakableCC);
                } else {
                    let midpoint = (my_pos + trap_target_info.position) / 2.0;
                    if try_place_trap_at(
                        commands, combat_log, abilities, entity, combatant, my_pos, midpoint,
                        TrapType::Freezing, &mut builder,
                    ) {
                        builder.finish();
                        return true;
                    }
                }
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

fn is_target_slowed(target: Entity, ctx: &CombatContext) -> bool {
    ctx.active_auras.get(&target).map_or(false, |auras| {
        auras.iter().any(|a| a.effect_type == AuraType::MovementSpeedSlow)
    })
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

    if is_target_slowed(target_entity, ctx) {
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
    // 100% -> 34% without this floor).
    const STING_MANA_FLOOR: f32 = 100.0;
    if combatant.current_mana < STING_MANA_FLOOR {
        builder.reject(ability, RejectionReason::PreconditionUnmet {
            note: "mana reserved for kiting toolkit".to_string(),
        });
        return false;
    }

    // Never sting a rage user: Warriors convert 15% of damage taken into rage
    // (auto_attack.rs / auras.rs), so a permanent DoT is a steady rage faucet
    // funding extra Mortal Strikes against our team. Sweep data: stinging
    // Warriors was uniquely immune to every other mitigation.
    if target_info.class == CharacterClass::Warrior {
        builder.reject(ability, RejectionReason::PreconditionUnmet {
            note: "sting feeds Warrior rage".to_string(),
        });
        return false;
    }

    // Trap-candidate reservation: don't sting the Freezing Trap candidate while
    // the trap is ready — a ticking sting would suppress the trap permanently
    // via the friendly-DoT guard (the kill target is often the enemy healer,
    // which is exactly who the trap wants). Once the trap is on cooldown the
    // sting resumes freely. Reactive state check, not predictive sequencing.
    let trap_ready = !combatant.ability_cooldowns.contains_key(&AbilityType::FreezingTrap)
        && abilities.get(&AbilityType::FreezingTrap)
            .is_some_and(|trap_def| combatant.current_mana >= trap_def.mana_cost);
    if trap_ready && ctx.enemy_healer().or(Some(target_entity)) == Some(target_entity) {
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
