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

    for (entity, mut combatant, transform, pet, auras) in pets.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }

        // Pets can't act while incapacitated
        let is_incapacitated = auras
            .map(|a| a.auras.iter().any(|aura| {
                matches!(aura.effect_type, AuraType::Stun | AuraType::Fear | AuraType::Polymorph)
            }))
            .unwrap_or(false);

        if is_incapacitated {
            continue;
        }

        let my_pos = transform.translation;
        let ctx = CombatContext {
            combatants: &combatant_info,
            active_auras: &active_auras_map,
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
    });

    true
}
