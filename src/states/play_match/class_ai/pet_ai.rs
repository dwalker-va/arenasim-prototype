//! Pet AI System
//!
//! Handles AI decisions for pet entities (Felhunter, etc.).
//! Runs separately from class AI - pets are skipped in the main dispatch loop
//! and processed here instead.
//!
//! ## Felhunter Priority
//! 1. Spell Lock (interrupt enemy casts within 30yd, 30s CD)
//! 2. Devour Magic (dispel debuffs from allies within 30yd, 8s CD)
//! 3. Auto-attack (handled by combat_auto_attack, not here)

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::class_ai::priest::DispelPending;
use crate::states::play_match::components::*;
use crate::states::play_match::utils::spawn_speech_bubble;
use crate::states::match_config::CharacterClass;
use super::CombatContext;

/// Pet AI decision system. Runs after class AI for non-pet combatants.
pub fn pet_ai_system(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    abilities: Res<AbilityDefinitions>,
    mut pets: Query<
        (Entity, &mut Combatant, &Transform, &Pet, Option<&ActiveAuras>),
        (Without<CastingState>, Without<ChannelingState>),
    >,
    casting_targets: Query<(Entity, &Combatant, &CastingState), Without<Pet>>,
    channeling_targets: Query<(Entity, &Combatant, &ChannelingState), (Without<CastingState>, Without<Pet>)>,
    all_combatants: Query<(Entity, &Combatant, &Transform, Option<&ActiveAuras>), Without<Pet>>,
    dr_tracker_query: Query<(Entity, &DRTracker)>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    if celebration.is_some() {
        return;
    }

    // Build CombatantInfo snapshot (same pattern as decide_abilities)
    let combatant_info: std::collections::HashMap<Entity, super::CombatantInfo> = all_combatants
        .iter()
        .map(|(entity, combatant, transform, _)| {
            (entity, super::CombatantInfo {
                entity,
                team: combatant.team,
                slot: combatant.slot,
                class: combatant.class,
                current_health: combatant.current_health,
                max_health: combatant.max_health,
                current_mana: combatant.current_mana,
                max_mana: combatant.max_mana,
                position: transform.translation,
                is_alive: combatant.is_alive(),
                stealthed: combatant.stealthed,
                target: combatant.target,
                is_pet: false, // We identify pets via the Pet component
                pet_type: None,
            })
        })
        .collect();

    let active_auras_map: std::collections::HashMap<Entity, Vec<Aura>> = all_combatants
        .iter()
        .filter_map(|(entity, _, _, auras_opt)| {
            auras_opt.map(|auras| (entity, auras.auras.clone()))
        })
        .collect();

    let dr_trackers: std::collections::HashMap<Entity, DRTracker> = dr_tracker_query
        .iter()
        .map(|(entity, tracker)| (entity, tracker.clone()))
        .collect();

    for (entity, mut combatant, transform, pet, auras) in pets.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }

        // Pets can't act while incapacitated
        let is_incapacitated = crate::states::play_match::utils::is_incapacitated(auras);

        if is_incapacitated {
            continue;
        }

        let my_pos = transform.translation;
        let ctx = CombatContext {
            combatants: &combatant_info,
            active_auras: &active_auras_map,
            dr_trackers: &dr_trackers,
            self_entity: entity,
        };

        match pet.pet_type {
            PetType::Felhunter => {
                felhunter_ai(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    entity,
                    &mut combatant,
                    my_pos,
                    &ctx,
                    &casting_targets,
                    &channeling_targets,
                );
            }
            PetType::Spider => {
                spider_ai(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    entity,
                    &mut combatant,
                    my_pos,
                    pet,
                    &ctx,
                );
            }
            PetType::Boar => {
                boar_ai(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    entity,
                    &mut combatant,
                    my_pos,
                    pet,
                    &ctx,
                    &casting_targets,
                );
            }
            PetType::Bird => {
                bird_ai(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    entity,
                    &mut combatant,
                    my_pos,
                    pet,
                    &ctx,
                );
            }
        }
    }
}

/// Felhunter AI priorities:
/// 1. Spell Lock - interrupt enemy casts (highest priority)
/// 2. Devour Magic - dispel debuffs from allies
fn felhunter_ai(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    casting_targets: &Query<(Entity, &Combatant, &CastingState), Without<Pet>>,
    channeling_targets: &Query<(Entity, &Combatant, &ChannelingState), (Without<CastingState>, Without<Pet>)>,
) {
    // On GCD — can't act
    if combatant.global_cooldown > 0.0 {
        return;
    }

    // Priority 1: Spell Lock (interrupt)
    if try_spell_lock(commands, combat_log, abilities, entity, combatant, my_pos, ctx, casting_targets, channeling_targets) {
        return;
    }

    // Priority 2: Devour Magic (dispel ally debuffs)
    if try_devour_magic(commands, combat_log, abilities, entity, combatant, my_pos, ctx) {
        return;
    }

    // Auto-attack is handled by combat_auto_attack system
}

/// Try to interrupt an enemy cast with Spell Lock.
fn try_spell_lock(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
    casting_targets: &Query<(Entity, &Combatant, &CastingState), Without<Pet>>,
    channeling_targets: &Query<(Entity, &Combatant, &ChannelingState), (Without<CastingState>, Without<Pet>)>,
) -> bool {
    let ability = AbilityType::SpellLock;
    let def = abilities.get_unchecked(&ability);

    // Check cooldown
    if combatant.ability_cooldowns.contains_key(&ability) {
        return false;
    }

    let my_team = combatant.team;

    // Find an enemy that is casting or channeling within range
    // Check casters first
    for (target_entity, target_combatant, cast_state) in casting_targets.iter() {
        if target_combatant.team == my_team || !target_combatant.is_alive() {
            continue;
        }

        if cast_state.interrupted {
            continue; // Already interrupted
        }

        if ctx.entity_is_immune(target_entity) {
            continue;
        }

        let distance = my_pos.distance(ctx.combatants.get(&target_entity)
            .map(|i| i.position)
            .unwrap_or(Vec3::ZERO));

        if distance > def.range {
            continue;
        }

        execute_spell_lock(commands, combat_log, abilities, entity, combatant, target_entity, &def.name);
        return true;
    }

    // Check channeling targets
    for (target_entity, target_combatant, _) in channeling_targets.iter() {
        if target_combatant.team == my_team || !target_combatant.is_alive() {
            continue;
        }

        if ctx.entity_is_immune(target_entity) {
            continue;
        }

        let distance = my_pos.distance(ctx.combatants.get(&target_entity)
            .map(|i| i.position)
            .unwrap_or(Vec3::ZERO));

        if distance > def.range {
            continue;
        }

        execute_spell_lock(commands, combat_log, abilities, entity, combatant, target_entity, &def.name);
        return true;
    }

    false
}

fn execute_spell_lock(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    target_entity: Entity,
    ability_name: &str,
) {
    let ability = AbilityType::SpellLock;
    let def = abilities.get_unchecked(&ability);

    combatant.ability_cooldowns.insert(ability, def.cooldown);
    // Interrupts don't trigger GCD in WoW

    let caster_id = format!("Team {} Felhunter", combatant.team);
    combat_log.log_ability_cast(
        caster_id,
        ability_name.to_string(),
        None,
        format!("Team {} Felhunter uses {}", combatant.team, ability_name),
    );

    spawn_speech_bubble(commands, entity, ability_name);

    commands.spawn(InterruptPending {
        caster: entity,
        target: target_entity,
        ability,
        lockout_duration: def.lockout_duration,
    });
}

/// Try to dispel a debuff from an ally with Devour Magic.
fn try_devour_magic(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    ctx: &CombatContext,
) -> bool {
    let ability = AbilityType::DevourMagic;
    let def = abilities.get_unchecked(&ability);

    // Check cooldown
    if combatant.ability_cooldowns.contains_key(&ability) {
        return false;
    }

    let my_team = combatant.team;

    // Find an ally with a dispellable debuff (prioritize primary allies, then self)
    let mut best_target: Option<(Entity, Vec3)> = None;

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != my_team || !info.is_alive {
            continue;
        }

        let distance = my_pos.distance(info.position);
        if distance > def.range {
            continue;
        }

        // Check if ally has any dispellable debuffs
        let has_dispellable = ctx.active_auras
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.can_be_dispelled()))
            .unwrap_or(false);

        if !has_dispellable {
            continue;
        }

        // Prefer primary allies over pets for dispels
        match best_target {
            None => best_target = Some((*ally_entity, info.position)),
            Some(_) if !info.is_pet => {
                best_target = Some((*ally_entity, info.position));
            }
            _ => {}
        }
    }

    let Some((target_entity, _)) = best_target else {
        return false;
    };

    // Use Devour Magic!
    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = super::super::constants::GCD;

    let caster_id = format!("Team {} Felhunter", combatant.team);
    combat_log.log_ability_cast(
        caster_id,
        def.name.to_string(),
        None,
        format!("Team {} Felhunter uses {}", combatant.team, def.name),
    );

    spawn_speech_bubble(commands, entity, &def.name);

    // Devour Magic heals the Felhunter for a percentage of its max HP on successful dispel
    let heal_amount = combatant.max_health * 0.10; // 10% of Felhunter max HP

    commands.spawn(DispelPending {
        target: target_entity,
        log_prefix: "[DEVOUR]",
        caster_class: CharacterClass::Warlock,
        heal_on_success: Some((entity, heal_amount)),
        aura_type_filter: None,
    });

    true
}

// ==============================================================================
// Spider AI
// ==============================================================================

/// Spider AI: Use Web to root enemies approaching the owner.
fn spider_ai(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    pet: &Pet,
    ctx: &CombatContext,
) {
    if combatant.global_cooldown > 0.0 {
        return;
    }

    // Try Spider Web: root enemy closest to owner that is approaching
    let ability = AbilityType::SpiderWeb;
    let Some(def) = abilities.get(&ability) else { return };
    if combatant.ability_cooldowns.contains_key(&ability) {
        return;
    }

    // Find owner position
    let owner_pos = ctx.combatants.get(&pet.owner).map(|info| info.position);
    let Some(owner_pos) = owner_pos else { return };

    // Find enemy closest to owner that is within Web range of the spider
    let mut best_target: Option<(Entity, f32)> = None;
    for (target_entity, info) in ctx.combatants.iter() {
        if info.team == combatant.team || !info.is_alive || info.is_pet || info.stealthed {
            continue;
        }
        let dist_to_owner = info.position.distance(owner_pos);
        // Only use Web if enemy is within 15 yards of owner (approaching)
        if dist_to_owner > 15.0 {
            continue;
        }
        // Check spider is within Web range of target
        let dist_to_spider = my_pos.distance(info.position);
        if dist_to_spider > def.range {
            continue;
        }
        // Don't root already-rooted targets
        if let Some(auras) = ctx.active_auras.get(target_entity) {
            if auras.iter().any(|a| a.effect_type == AuraType::Root) {
                continue;
            }
        }
        if best_target.map_or(true, |(_, d)| dist_to_owner < d) {
            best_target = Some((*target_entity, dist_to_owner));
        }
    }

    let Some((target_entity, _)) = best_target else { return };

    // Spawn projectile — root aura applied on impact by process_projectile_hits
    let projectile_speed = def.projectile_speed.unwrap_or(50.0);
    commands.spawn((
        Projectile {
            caster: entity,
            target: target_entity,
            ability,
            speed: projectile_speed,
            caster_team: combatant.team,
            caster_class: combatant.class,
        },
        Transform::from_translation(my_pos + Vec3::new(0.0, 0.5, 0.0)),
        PlayMatchEntity,
    ));

    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = super::super::constants::GCD;

    let caster_id = format!("Team {} Spider", combatant.team);
    combat_log.log_ability_cast(
        caster_id,
        def.name.to_string(),
        None,
        format!("Team {} Spider uses {}", combatant.team, def.name),
    );

    spawn_speech_bubble(commands, entity, &def.name);
}

// ==============================================================================
// Boar AI
// ==============================================================================

/// Boar AI: Charge enemies mid-cast or the kill target.
fn boar_ai(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    pet: &Pet,
    ctx: &CombatContext,
    casting_targets: &Query<(Entity, &Combatant, &CastingState), Without<Pet>>,
) {
    if combatant.global_cooldown > 0.0 {
        return;
    }

    let ability = AbilityType::BoarCharge;
    let Some(def) = abilities.get(&ability) else { return };
    if combatant.ability_cooldowns.contains_key(&ability) {
        return;
    }

    // Priority 1: Charge enemy mid-cast (especially healers)
    let mut charge_target: Option<Entity> = None;
    for (target_entity, target_combatant, _cast_state) in casting_targets.iter() {
        if target_combatant.team == combatant.team || !target_combatant.is_alive() || target_combatant.stealthed {
            continue;
        }
        if let Some(info) = ctx.combatants.get(&target_entity) {
            let dist = my_pos.distance(info.position);
            if dist >= super::super::constants::CHARGE_MIN_RANGE && dist <= def.range {
                charge_target = Some(target_entity);
                break;
            }
        }
    }

    // Priority 2: Charge owner's target
    if charge_target.is_none() {
        if let Some(owner_info) = ctx.combatants.get(&pet.owner) {
            if let Some(owner_target) = owner_info.target {
                if let Some(target_info) = ctx.combatants.get(&owner_target) {
                    if target_info.is_alive && target_info.team != combatant.team {
                        let dist = my_pos.distance(target_info.position);
                        if dist >= super::super::constants::CHARGE_MIN_RANGE && dist <= def.range {
                            charge_target = Some(owner_target);
                        }
                    }
                }
            }
        }
    }

    let Some(target) = charge_target else { return };

    // Start charging (use ChargingState like Warrior Charge)
    commands.entity(entity).try_insert(ChargingState { target });

    // Apply stun via AuraPending
    if let Some(aura_def) = &def.applies_aura {
        commands.spawn((
            AuraPending {
                target,
                aura: Aura {
                    effect_type: aura_def.aura_type,
                    duration: aura_def.duration,
                    magnitude: aura_def.magnitude,
                    tick_interval: 0.0,
                    time_until_next_tick: 0.0,
                    caster: Some(entity),
                    ability_name: def.name.to_string(),
                    break_on_damage_threshold: aura_def.break_on_damage,
                    accumulated_damage: 0.0,
                    fear_direction: (0.0, 0.0),
                    fear_direction_timer: 0.0,
                    spell_school: Some(def.spell_school),
                },
            },
            PlayMatchEntity,
        ));
    }

    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = super::super::constants::GCD;

    let caster_id = format!("Team {} Boar", combatant.team);
    combat_log.log_ability_cast(
        caster_id,
        def.name.to_string(),
        None,
        format!("Team {} Boar uses {}", combatant.team, def.name),
    );

    spawn_speech_bubble(commands, entity, &def.name);
}

// ==============================================================================
// Bird AI
// ==============================================================================

/// Bird AI: Master's Call to remove movement impairments from owner/allies.
fn bird_ai(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    _my_pos: Vec3,
    pet: &Pet,
    ctx: &CombatContext,
) {
    if combatant.global_cooldown > 0.0 {
        return;
    }

    let ability = AbilityType::MastersCall;
    let Some(def) = abilities.get(&ability) else { return };
    if combatant.ability_cooldowns.contains_key(&ability) {
        return;
    }

    // Priority 1: Use when owner has Root or MovementSpeedSlow
    let owner_needs_cleanse = ctx.active_auras.get(&pet.owner).map_or(false, |auras| {
        auras.iter().any(|a| matches!(
            a.effect_type,
            AuraType::Root | AuraType::MovementSpeedSlow
        ))
    });

    let cleanse_target = if owner_needs_cleanse {
        Some(pet.owner)
    } else {
        // Priority 2: Use on teammate with movement impairments
        let mut fallback: Option<Entity> = None;
        for (ally_entity, info) in ctx.combatants.iter() {
            if info.team != combatant.team || !info.is_alive || info.is_pet {
                continue;
            }
            if let Some(auras) = ctx.active_auras.get(ally_entity) {
                if auras.iter().any(|a| matches!(
                    a.effect_type,
                    AuraType::Root | AuraType::MovementSpeedSlow
                )) {
                    fallback = Some(*ally_entity);
                    break;
                }
            }
        }
        fallback
    };

    let Some(target) = cleanse_target else { return };

    // Master's Call: only removes movement impairments (Root, MovementSpeedSlow)
    commands.spawn(DispelPending {
        target,
        log_prefix: "[MASTERS_CALL]",
        caster_class: CharacterClass::Hunter,
        heal_on_success: None,
        aura_type_filter: Some(vec![AuraType::Root, AuraType::MovementSpeedSlow]),
    });

    // Spawn golden burst visual on the cleanse target
    commands.spawn((
        DispelBurst {
            target,
            caster_class: CharacterClass::Hunter,
            lifetime: 0.3,
            initial_lifetime: 0.3,
        },
        PlayMatchEntity,
    ));

    combatant.ability_cooldowns.insert(ability, def.cooldown);
    combatant.global_cooldown = super::super::constants::GCD;

    let caster_id = format!("Team {} Bird", combatant.team);
    combat_log.log_ability_cast(
        caster_id,
        def.name.to_string(),
        None,
        format!("Team {} Bird uses {}", combatant.team, def.name),
    );

    spawn_speech_bubble(commands, entity, &def.name);
}
