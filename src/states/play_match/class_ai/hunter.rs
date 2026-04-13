//! Hunter AI — Ranged physical DPS with pet, traps, and dead zone management.
//!
//! The Hunter prioritizes maintaining distance and controlling space.
//! Key mechanics: dead zone (can't use ranged abilities within 8 yards),
//! kiting, trap placement, and pet coordination.
//!
//! ## Range Zone Priorities
//! - **Dead zone (<8 yards)**: Disengage > Frost Trap at feet > Kite
//! - **Closing (8-20 yards)**: Concussive Shot > Frost Trap > Kite + Arcane Shot
//! - **Safe (20-40 yards)**: Concussive Shot > Aimed Shot > Arcane Shot
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::constants::*;
use crate::states::play_match::is_spell_school_locked;

use super::CombatContext;
use super::super::utils::log_ability_use;

/// Hunter AI: Decides and executes abilities for a Hunter combatant.
///
/// Returns `true` if an action was taken this frame.
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
) -> bool {
    // Find nearest enemy and distance
    let (nearest_enemy, nearest_distance) = find_nearest_enemy(entity, combatant.team, my_pos, ctx);

    // Kite if a melee enemy is within kite range (regardless of GCD)
    // Don't kite ranged classes — two Hunters shouldn't flee from each other
    let nearest_dist = nearest_distance.unwrap_or(40.0);
    let nearest_is_melee = nearest_enemy
        .and_then(|(e, _)| ctx.combatants.get(&e))
        .map_or(false, |info| info.class.is_melee());
    if nearest_is_melee && nearest_dist < HUNTER_KITE_RANGE {
        combatant.kiting_timer = combatant.kiting_timer.max(0.5);
    }

    // Check if global cooldown is active
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    // Get primary target info
    let Some(target_entity) = combatant.target else {
        return false;
    };
    let Some(target_info) = ctx.combatants.get(&target_entity) else {
        return false;
    };
    if !target_info.is_alive {
        return false;
    }
    // Don't waste abilities on immune targets (Divine Shield)
    if ctx.entity_is_immune(target_entity) {
        return false;
    }
    let distance_to_target = my_pos.distance(target_info.position);

    // === DEAD ZONE (<8 yards) — Escape priority ===
    if nearest_dist < HUNTER_DEAD_ZONE {
        // Check if rooted (can't Disengage while rooted)
        let is_rooted = auras.map_or(false, |a| {
            a.auras.iter().any(|aura| aura.effect_type == AuraType::Root)
        });

        // Priority 1: Disengage (if off cooldown and not rooted)
        if !is_rooted {
            if let Some(def) = abilities.get(&AbilityType::Disengage) {
                if !combatant.ability_cooldowns.contains_key(&AbilityType::Disengage)
                    && combatant.current_mana >= def.mana_cost
                {
                    // Calculate backward direction (away from nearest enemy)
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
                        // Fallback: move toward own team's side
                        if combatant.team == 1 { Vec3::new(-1.0, 0.0, 0.0) } else { Vec3::new(1.0, 0.0, 0.0) }
                    } else {
                        Vec3::new(away_dir.x, 0.0, away_dir.z).normalize_or_zero()
                    };

                    commands.entity(entity).try_insert(DisengagingState {
                        direction,
                        distance_remaining: DISENGAGE_DISTANCE,
                    });

                    combatant.current_mana -= def.mana_cost;
                    combatant.ability_cooldowns.insert(AbilityType::Disengage, def.cooldown);
                    combatant.global_cooldown = GCD;

                    log_ability_use(combat_log, combatant.team, combatant.class, "Disengage", None, "uses");
                    return true;
                }
            }
        }

        // Priority 2: Frost Trap at current position
        if try_place_trap_at(commands, combat_log, abilities, entity, combatant, my_pos, my_pos, TrapType::Frost) {
            return true;
        }

        // Priority 3: Set kiting timer to flee
        combatant.kiting_timer = 3.0;
        return false; // Let movement system handle fleeing
    }

    // === CLOSING RANGE (8-20 yards) — Kite + instants ===
    if nearest_dist < 20.0 {
        // Priority 1: Concussive Shot on nearest enemy (if not already slowed)
        if let Some((enemy_entity, _)) = nearest_enemy {
            if try_concussive_shot(commands, combat_log, abilities, entity, combatant, my_pos, enemy_entity, ctx) {
                combatant.kiting_timer = 3.0; // Kite after slowing
                return true;
            }
        }

        // Priority 2: Frost Trap between self and nearest enemy
        if let Some((enemy_entity, _)) = nearest_enemy {
            if let Some(enemy_info) = ctx.combatants.get(&enemy_entity) {
                let midpoint = (my_pos + enemy_info.position) / 2.0;
                if try_place_trap_at(commands, combat_log, abilities, entity, combatant, my_pos, midpoint, TrapType::Frost) {
                    combatant.kiting_timer = 3.0;
                    return true;
                }
            }
        }

        // Priority 3: Arcane Shot while kiting (instant, decent damage)
        if try_arcane_shot(commands, combat_log, game_rng, abilities, entity, combatant, my_pos, target_entity, target_info, ctx, instant_attacks, auras) {
            combatant.kiting_timer = 3.0;
            return true;
        }

        // Set kiting timer regardless
        combatant.kiting_timer = 3.0;
        return false;
    }

    // === SAFE RANGE (20+ yards) — Full rotation ===

    // Priority 1: Concussive Shot (if target not slowed)
    if try_concussive_shot(commands, combat_log, abilities, entity, combatant, my_pos, target_entity, ctx) {
        return true;
    }

    // Priority 2: Freezing Trap on healer/CC target (or primary target in 1v1)
    let freezing_trap_target = find_enemy_healer(combatant.team, ctx)
        .or(Some(target_entity));
    if let Some(trap_target) = freezing_trap_target {
        if let Some(trap_target_info) = ctx.combatants.get(&trap_target) {
            if trap_target_info.is_alive {
                // Place between self and target for interception
                let midpoint = (my_pos + trap_target_info.position) / 2.0;
                if try_place_trap_at(commands, combat_log, abilities, entity, combatant, my_pos, midpoint, TrapType::Freezing) {
                    return true;
                }
            }
        }
    }

    // Priority 3: Aimed Shot (safe to hardcast at 20+ yards — ~2.4s before Warrior reaches dead zone)
    if distance_to_target >= 20.0 {
        if try_aimed_shot(commands, combat_log, abilities, entity, combatant, my_pos, target_entity, target_info, auras, ctx) {
            return true;
        }
    }

    // Priority 4: Arcane Shot (instant filler)
    if try_arcane_shot(commands, combat_log, game_rng, abilities, entity, combatant, my_pos, target_entity, target_info, ctx, instant_attacks, auras) {
        return true;
    }

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

fn find_enemy_healer(my_team: u8, ctx: &CombatContext) -> Option<Entity> {
    ctx.combatants.iter()
        .find(|(_, info)| {
            info.team != my_team
                && info.is_alive
                && !info.is_pet
                && info.class.is_healer()
        })
        .map(|(entity, _)| *entity)
}

fn is_target_slowed(target: Entity, ctx: &CombatContext) -> bool {
    ctx.active_auras.get(&target).map_or(false, |auras| {
        auras.iter().any(|a| a.effect_type == AuraType::MovementSpeedSlow)
    })
}

/// Attempt to place a trap at a specific position (or at the Hunter's feet).
/// If the target position is > TRAP_LAUNCH_MIN_RANGE from the hunter, spawns a
/// TrapLaunchProjectile that arcs to the target before the trap begins arming.
fn try_place_trap_at(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    position: Vec3,
    trap_type: TrapType,
) -> bool {
    let ability = match trap_type {
        TrapType::Freezing => AbilityType::FreezingTrap,
        TrapType::Frost => AbilityType::FrostTrap,
    };

    let Some(def) = abilities.get(&ability) else { return false };
    if combatant.ability_cooldowns.contains_key(&ability) {
        return false;
    }
    if combatant.current_mana < def.mana_cost {
        return false;
    }

    // Clamp to octagonal arena bounds (midpoint can land outside corners)
    let position = crate::states::play_match::combat_core::clamp_to_arena(position);

    let trap_name = trap_type.name();

    // Distance check uses XZ plane only (ignore Y)
    let distance = Vec3::new(my_pos.x, 0.0, my_pos.z)
        .distance(Vec3::new(position.x, 0.0, position.z));

    if distance > TRAP_LAUNCH_MIN_RANGE {
        // Launch: spawn arc projectile from Hunter position
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
        // Drop: instant spawn at target position (short range)
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
) -> bool {
    if ctx.has_friendly_breakable_cc(target_entity) {
        return false;
    }

    let ability = AbilityType::ConcussiveShot;
    let Some(def) = abilities.get(&ability) else { return false };
    if combatant.ability_cooldowns.contains_key(&ability) { return false }
    if combatant.current_mana < def.mana_cost { return false }

    let Some(target_info) = ctx.combatants.get(&target_entity) else { return false };
    let distance = my_pos.distance(target_info.position);

    // Check min range (dead zone)
    if let Some(min_range) = def.min_range {
        if distance < min_range { return false }
    }
    if distance > def.range { return false }

    // Don't use if target already slowed
    if is_target_slowed(target_entity, ctx) { return false }

    // Spawn projectile — aura applied on impact by process_projectile_hits
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
) -> bool {
    if ctx.has_friendly_breakable_cc(target_entity) {
        return false;
    }

    let ability = AbilityType::AimedShot;
    let Some(def) = abilities.get(&ability) else { return false };
    if combatant.ability_cooldowns.contains_key(&ability) { return false }
    if combatant.current_mana < def.mana_cost { return false }

    let distance = my_pos.distance(target_info.position);
    if let Some(min_range) = def.min_range {
        if distance < min_range { return false }
    }
    if distance > def.range { return false }

    // Start casting
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
) -> bool {
    if ctx.has_friendly_breakable_cc(target_entity) {
        return false;
    }

    let ability = AbilityType::ArcaneShot;
    let Some(def) = abilities.get(&ability) else { return false };
    if combatant.ability_cooldowns.contains_key(&ability) { return false }
    if combatant.current_mana < def.mana_cost { return false }
    if is_spell_school_locked(def.spell_school, auras) { return false }

    let distance = my_pos.distance(target_info.position);
    if let Some(min_range) = def.min_range {
        if distance < min_range { return false }
    }
    if distance > def.range { return false }

    // Spawn projectile — damage is calculated and applied on impact by process_projectile_hits
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

