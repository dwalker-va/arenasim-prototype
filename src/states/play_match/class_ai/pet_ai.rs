//! Pet AI System
//!
//! Handles AI decisions for pet entities (Felhunter, Spider, Boar, Bird).
//! Runs separately from class AI - pets are skipped in the main dispatch loop
//! and processed here instead.

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::decision_trace::{
    ActorView, DecisionEventBuilder, DecisionTrace, RejectionReason, TargetView,
};
use crate::states::play_match::utils::spawn_speech_bubble;
use crate::states::match_config::CharacterClass;
use super::CombatContext;

/// Render a PetType variant into a stable string for pet_decision events.
fn pet_type_str(pt: PetType) -> &'static str {
    match pt {
        PetType::Felhunter => "Felhunter",
        PetType::Spider => "Spider",
        PetType::Boar => "Boar",
        PetType::Bird => "Bird",
    }
}

/// Map a pet type to its headline ability — used for the Heel-mode trace
/// event's `reject` payload so the audit attributes the no-action to the
/// pet's primary capability.
fn headline_ability_for(pt: PetType) -> AbilityType {
    match pt {
        PetType::Felhunter => AbilityType::SpellLock,
        PetType::Spider => AbilityType::SpiderWeb,
        PetType::Boar => AbilityType::BoarCharge,
        PetType::Bird => AbilityType::MastersCall,
    }
}

/// Pet AI decision system.
pub fn pet_ai_system(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    abilities: Res<AbilityDefinitions>,
    mut pets: Query<
        (Entity, &mut Combatant, &Transform, &Pet, Option<&ActiveAuras>, Option<&PetCommand>),
        (Without<CastingState>, Without<ChannelingState>),
    >,
    casting_targets: Query<(Entity, &Combatant, &CastingState), Without<Pet>>,
    channeling_targets: Query<(Entity, &Combatant, &ChannelingState), (Without<CastingState>, Without<Pet>)>,
    all_combatants: Query<(Entity, &Combatant, &Transform, Option<&ActiveAuras>), Without<Pet>>,
    dr_tracker_query: Query<(Entity, &DRTracker)>,
    celebration: Option<Res<VictoryCelebration>>,
    mut decision_trace: ResMut<DecisionTrace>,
) {
    if celebration.is_some() {
        return;
    }

    // Owner→pet reverse lookup, populated from the mutable `pets` query via a
    // read-only `.iter()` pass (released before `.iter_mut()` in the main
    // loop). Matches the CombatSnapshot::build pattern in combat_snapshot.rs.
    let owner_to_pet: std::collections::BTreeMap<Entity, Entity> = pets
        .iter()
        .map(|(entity, _, _, pet, _, _)| (pet.owner, entity))
        .collect();

    let combatant_info: std::collections::BTreeMap<Entity, super::CombatantInfo> = all_combatants
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
                is_pet: false,
                pet_type: None,
                pet: owner_to_pet.get(&entity).copied(),
            })
        })
        .collect();

    let active_auras_map: std::collections::BTreeMap<Entity, Vec<Aura>> = all_combatants
        .iter()
        .filter_map(|(entity, _, _, auras_opt)| {
            auras_opt.map(|auras| (entity, auras.auras.clone()))
        })
        .collect();

    let dr_trackers: std::collections::BTreeMap<Entity, DRTracker> = dr_tracker_query
        .iter()
        .map(|(entity, tracker)| (entity, tracker.clone()))
        .collect();

    // Per-entity ability cooldowns snapshot (BTreeMap for determinism). Pet AI
    // doesn't currently read this from `ctx`, but keeping it consistent with
    // CombatSnapshot::build avoids drift if future pet AI code reads cooldowns.
    let ability_cooldowns: std::collections::BTreeMap<Entity, std::collections::BTreeMap<crate::states::play_match::abilities::AbilityType, f32>> =
        all_combatants
            .iter()
            .map(|(entity, combatant, _, _)| {
                let cds: std::collections::BTreeMap<_, _> = combatant
                    .ability_cooldowns
                    .iter()
                    .map(|(k, v)| (*k, *v))
                    .collect();
                (entity, cds)
            })
            .collect();

    for (entity, mut combatant, transform, pet, auras, pet_command) in pets.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }

        let is_incapacitated = crate::states::play_match::utils::is_incapacitated(auras);
        if is_incapacitated {
            // Despawn any queued PetCommand so it doesn't fire next tick.
            if pet_command.is_some() {
                commands.entity(entity).remove::<PetCommand>();
            }
            continue;
        }

        // U1: Pet target ownership. Pets no longer receive target assignments
        // from `acquire_targets` (per the pet-skip at combat_ai.rs around line
        // ~107). Pet AI assigns pet.target = owner.target so existing
        // target-pursuit movement (movement.rs:391+) closes pets on enemies.
        //
        // U6: Heel predicate — when HP < 25%, target is cleared, any queued
        // PetCommand is despawned, and the pet returns to the owner's flank
        // via the existing follow-owner branch (movement.rs:309+). A
        // LowHealthHeel rejection trace event is emitted so the audit can
        // attribute the no-action to the predicate.
        let hp_ratio = if combatant.max_health > 0.0 {
            combatant.current_health / combatant.max_health
        } else {
            0.0
        };
        let in_heel = hp_ratio < 0.25;
        if in_heel {
            combatant.target = None;
            // Despawn any Hunter-dispatched PetCommand without execution.
            if pet_command.is_some() {
                commands.entity(entity).remove::<PetCommand>();
            }
            // Emit a pet_decision trace event with reject(headline, LowHealthHeel)
            // so the audit attributes the no-action correctly. Headline ability
            // selection is per-pet-type to match what would otherwise be the
            // pet's first try_* candidate.
            let headline = headline_ability_for(pet.pet_type);
            let hp_pct = hp_ratio;
            let mana_pct = if combatant.max_mana > 0.0 {
                combatant.current_mana / combatant.max_mana
            } else {
                0.0
            };
            let actor_view = ActorView::from_raw(
                entity,
                combatant.team,
                combatant.slot,
                combatant.class,
                hp_pct,
                mana_pct,
                transform.translation,
            );
            let mut builder = decision_trace.start_pet_decision(
                actor_view,
                None,
                pet.owner,
                pet_type_str(pet.pet_type),
            );
            builder.reject(headline, RejectionReason::LowHealthHeel);
            builder.finish();
            continue;
        } else {
            combatant.target = combatant_info.get(&pet.owner).and_then(|owner_info| owner_info.target);
        }

        // PetCommand handling: U3 wires the component; full Hunter-dispatch
        // execution lands with U4. For now, consume any queued command (despawn
        // without execution) so the framework is in place. The autonomous
        // spider_ai/boar_ai/bird_ai/felhunter_ai paths still drive ability
        // selection until U4 replaces them with Hunter-side dispatch.
        if pet_command.is_some() {
            commands.entity(entity).remove::<PetCommand>();
        }

        let my_pos = transform.translation;
        let ctx = CombatContext {
            combatants: &combatant_info,
            active_auras: &active_auras_map,
            dr_trackers: &dr_trackers,
            ability_cooldowns: &ability_cooldowns,
            self_entity: entity,
        };

        // Build an ActorView for the pet. Pets don't appear in combatant_info
        // (which is non-pet only), so we synthesize one from raw fields.
        let hp_pct = if combatant.max_health > 0.0 {
            combatant.current_health / combatant.max_health
        } else {
            0.0
        };
        let mana_pct = if combatant.max_mana > 0.0 {
            combatant.current_mana / combatant.max_mana
        } else {
            0.0
        };
        let actor_view = ActorView::from_raw(
            entity,
            combatant.team,
            combatant.slot,
            combatant.class,
            hp_pct,
            mana_pct,
            my_pos,
        );
        let target_view = combatant
            .target
            .and_then(|t| ctx.combatants.get(&t))
            .map(|info| TargetView::from_info(info, my_pos));

        let mut builder = decision_trace.start_pet_decision(
            actor_view,
            target_view,
            pet.owner,
            pet_type_str(pet.pet_type),
        );

        match pet.pet_type {
            PetType::Felhunter => {
                felhunter_ai(
                    &mut commands, &mut combat_log, &abilities, entity, &mut combatant,
                    my_pos, &ctx, &casting_targets, &channeling_targets, &mut builder,
                );
            }
            PetType::Spider => {
                spider_ai(
                    &mut commands, &mut combat_log, &abilities, entity, &mut combatant,
                    my_pos, pet, &ctx, &mut builder,
                );
            }
            PetType::Boar => {
                boar_ai(
                    &mut commands, &mut combat_log, &abilities, entity, &mut combatant,
                    my_pos, pet, &ctx, &casting_targets, &mut builder,
                );
            }
            PetType::Bird => {
                bird_ai(
                    &mut commands, &mut combat_log, &abilities, entity, &mut combatant,
                    my_pos, pet, &ctx, &mut builder,
                );
            }
        }

        builder.finish();
    }
}

/// Felhunter AI priorities: Spell Lock then Devour Magic.
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
    builder: &mut DecisionEventBuilder<'_>,
) {
    if combatant.global_cooldown > 0.0 {
        return;
    }

    if try_spell_lock(commands, combat_log, abilities, entity, combatant, my_pos, ctx, casting_targets, channeling_targets, builder) {
        return;
    }

    if try_devour_magic(commands, combat_log, abilities, entity, combatant, my_pos, ctx, builder) {
        return;
    }
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
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::SpellLock;
    let def = abilities.get_unchecked(&ability);

    if let Some(remaining) = combatant.ability_cooldowns.get(&ability) {
        builder.reject(ability, RejectionReason::OnCooldown { remaining: *remaining });
        return false;
    }

    let my_team = combatant.team;

    for (target_entity, target_combatant, cast_state) in casting_targets.iter() {
        if target_combatant.team == my_team || !target_combatant.is_alive() {
            continue;
        }
        if cast_state.interrupted {
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
        builder.choose(ability, Some(target_entity), true);
        execute_spell_lock(commands, combat_log, abilities, entity, combatant, target_entity, &def.name);
        return true;
    }

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
        builder.choose(ability, Some(target_entity), true);
        execute_spell_lock(commands, combat_log, abilities, entity, combatant, target_entity, &def.name);
        return true;
    }

    builder.reject(ability, RejectionReason::NoValidTarget);
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
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::DevourMagic;
    let def = abilities.get_unchecked(&ability);

    if let Some(remaining) = combatant.ability_cooldowns.get(&ability) {
        builder.reject(ability, RejectionReason::OnCooldown { remaining: *remaining });
        return false;
    }

    let my_team = combatant.team;
    let mut best_target: Option<(Entity, Vec3)> = None;

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != my_team || !info.is_alive {
            continue;
        }
        let distance = my_pos.distance(info.position);
        if distance > def.range {
            continue;
        }
        let has_dispellable = ctx.active_auras
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| a.can_be_dispelled()))
            .unwrap_or(false);
        if !has_dispellable {
            continue;
        }
        match best_target {
            None => best_target = Some((*ally_entity, info.position)),
            Some(_) if !info.is_pet => {
                best_target = Some((*ally_entity, info.position));
            }
            _ => {}
        }
    }

    let Some((target_entity, _)) = best_target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return false;
    };

    builder.choose(ability, Some(target_entity), true);

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

    let heal_amount = combatant.max_health * 0.10;

    commands.spawn(DispelPending {
        target: target_entity,
        dispeller: entity,
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

fn spider_ai(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    pet: &Pet,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) {
    if combatant.global_cooldown > 0.0 {
        return;
    }

    let ability = AbilityType::SpiderWeb;
    let Some(def) = abilities.get(&ability) else { return };
    if let Some(remaining) = combatant.ability_cooldowns.get(&ability) {
        builder.reject(ability, RejectionReason::OnCooldown { remaining: *remaining });
        return;
    }

    // Owner presence is still required (pet without an owner shouldn't dispatch);
    // owner position is no longer used now that the dist_to_owner filter is gone.
    if ctx.combatants.get(&pet.owner).is_none() {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    }

    // U5: dist_to_owner ≤ 15.0 filter removed. The Spider's only spatial
    // constraint is its own ability range (currently 20yd) against the target.
    // The filter was incompatible with Hunter's 35yd kit range — when Hunter
    // kited at safe distance, no target ever passed the 15yd-from-owner check
    // and Spider Web never fired in 1v1.
    let mut best_target: Option<(Entity, f32)> = None;
    for (target_entity, info) in ctx.combatants.iter() {
        if info.team == combatant.team || !info.is_alive || info.is_pet || info.stealthed {
            continue;
        }
        let dist_to_spider = my_pos.distance(info.position);
        if dist_to_spider > def.range {
            continue;
        }
        if let Some(auras) = ctx.active_auras.get(target_entity) {
            if auras.iter().any(|a| a.effect_type == AuraType::Root) {
                continue;
            }
        }
        // Prefer the closer-to-spider target (was: closer-to-owner under the
        // old filter). Since the Spider stays near the owner today, this is
        // equivalent in practice but more correct in spirit.
        if best_target.map_or(true, |(_, d)| dist_to_spider < d) {
            best_target = Some((*target_entity, dist_to_spider));
        }
    }

    let Some((target_entity, _)) = best_target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    };

    builder.choose(ability, Some(target_entity), true);

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
    builder: &mut DecisionEventBuilder<'_>,
) {
    if combatant.global_cooldown > 0.0 {
        return;
    }

    let ability = AbilityType::BoarCharge;
    let Some(def) = abilities.get(&ability) else { return };
    if let Some(remaining) = combatant.ability_cooldowns.get(&ability) {
        builder.reject(ability, RejectionReason::OnCooldown { remaining: *remaining });
        return;
    }

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

    let Some(target) = charge_target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    };

    builder.choose(ability, Some(target), true);

    commands.entity(entity).try_insert(ChargingState { target });

    if let Some(aura_pending) = AuraPending::from_ability(target, entity, def) {
        commands.spawn((aura_pending, PlayMatchEntity));
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

fn bird_ai(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    _my_pos: Vec3,
    pet: &Pet,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) {
    if combatant.global_cooldown > 0.0 {
        return;
    }

    let ability = AbilityType::MastersCall;
    let Some(def) = abilities.get(&ability) else { return };
    if let Some(remaining) = combatant.ability_cooldowns.get(&ability) {
        builder.reject(ability, RejectionReason::OnCooldown { remaining: *remaining });
        return;
    }

    let owner_needs_cleanse = ctx.active_auras.get(&pet.owner).map_or(false, |auras| {
        auras.iter().any(|a| matches!(
            a.effect_type,
            AuraType::Root | AuraType::MovementSpeedSlow
        ))
    });

    let cleanse_target = if owner_needs_cleanse {
        Some(pet.owner)
    } else {
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

    let Some(target) = cleanse_target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    };

    builder.choose(ability, Some(target), true);

    commands.spawn(DispelPending {
        target,
        dispeller: entity,
        log_prefix: "[MASTERS_CALL]",
        caster_class: CharacterClass::Hunter,
        heal_on_success: None,
        aura_type_filter: Some(vec![AuraType::Root, AuraType::MovementSpeedSlow]),
    });

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
