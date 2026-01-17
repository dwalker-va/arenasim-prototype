//! Rogue AI Module
//!
//! Handles AI decision-making for the Rogue class.
//!
//! ## Priority Order (Stealthed)
//! 1. Ambush (opener from stealth)
//!
//! ## Priority Order (In Combat)
//! 1. Kidney Shot (stun)
//! 2. Sinister Strike (combo point builder)

use bevy::prelude::*;
use std::collections::HashMap;

use crate::combat::log::CombatLog;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::GCD;
use crate::states::play_match::utils::{combatant_id, spawn_speech_bubble};

use super::{AbilityDecision, ClassAI, CombatContext};

/// Rogue AI implementation.
///
/// Note: Currently uses direct execution via `decide_rogue_action()`.
/// The trait implementation is a stub for future refactoring.
pub struct RogueAI;

impl ClassAI for RogueAI {
    fn decide_action(&self, _ctx: &CombatContext, _combatant: &Combatant) -> AbilityDecision {
        // TODO: Migrate to trait-based decision making
        // For now, use decide_rogue_action() directly from combat_ai.rs
        AbilityDecision::None
    }
}

/// Rogue AI: Decides and executes abilities for a Rogue combatant.
///
/// Returns `true` if an action was taken this frame (caller should skip to next combatant).
#[allow(clippy::too_many_arguments)]
pub fn decide_rogue_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    positions: &HashMap<Entity, Vec3>,
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
    instant_attacks: &mut Vec<(Entity, Entity, f32, u8, CharacterClass, AbilityType)>,
) -> bool {
    // Get target
    let Some(target_entity) = combatant.target else {
        return false;
    };

    let Some(&target_pos) = positions.get(&target_entity) else {
        return false;
    };

    if combatant.stealthed {
        // Stealthed: Use Ambush
        return try_ambush(
            combat_log,
            game_rng,
            abilities,
            entity,
            combatant,
            my_pos,
            target_entity,
            target_pos,
            combatant_info,
            instant_attacks,
        );
    }

    // Not stealthed: Check GCD first
    if combatant.global_cooldown > 0.0 {
        return false;
    }

    // Priority 1: Kidney Shot
    if try_kidney_shot(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        target_entity,
        target_pos,
        combatant_info,
    ) {
        return true;
    }

    // Priority 2: Sinister Strike
    try_sinister_strike(
        combat_log,
        game_rng,
        abilities,
        entity,
        combatant,
        my_pos,
        target_entity,
        target_pos,
        combatant_info,
        instant_attacks,
    )
}

/// Try to use Ambush from stealth.
/// Returns true if Ambush was used.
#[allow(clippy::too_many_arguments)]
fn try_ambush(
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_pos: Vec3,
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
    instant_attacks: &mut Vec<(Entity, Entity, f32, u8, CharacterClass, AbilityType)>,
) -> bool {
    let ability = AbilityType::Ambush;

    if !ability.can_cast(combatant, target_pos, my_pos) {
        return false;
    }

    let def = abilities.get_unchecked(&ability);

    // Execute Ambush
    combatant.current_mana -= def.mana_cost;
    combatant.stealthed = false;
    combatant.global_cooldown = GCD;

    // Calculate and queue damage
    let damage = combatant.calculate_ability_damage_config(def, game_rng);
    instant_attacks.push((
        entity,
        target_entity,
        damage,
        combatant.team,
        combatant.class,
        ability,
    ));

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Ambush".to_string(),
        target_id,
        format!(
            "Team {} {} uses Ambush from stealth",
            combatant.team,
            combatant.class.name()
        ),
    );

    info!(
        "Team {} {} uses {} from stealth!",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to use Kidney Shot.
/// Returns true if Kidney Shot was used.
#[allow(clippy::too_many_arguments)]
fn try_kidney_shot(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_pos: Vec3,
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
) -> bool {
    let kidney_shot = AbilityType::KidneyShot;
    let ks_on_cooldown = combatant.ability_cooldowns.contains_key(&kidney_shot);

    if ks_on_cooldown {
        return false;
    }

    if !kidney_shot.can_cast(combatant, target_pos, my_pos) {
        return false;
    }

    let def = abilities.get_unchecked(&kidney_shot);

    // Execute Kidney Shot
    spawn_speech_bubble(commands, entity, "Kidney Shot");
    combatant.current_mana -= def.mana_cost;
    combatant.ability_cooldowns.insert(kidney_shot, def.cooldown);
    combatant.global_cooldown = GCD;

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Kidney Shot".to_string(),
        target_id.clone(),
        format!(
            "Team {} {} uses Kidney Shot",
            combatant.team,
            combatant.class.name()
        ),
    );

    // Apply stun aura
    if let Some(aura) = def.applies_aura.as_ref() {
        commands.spawn(AuraPending {
            target: target_entity,
            aura: Aura {
                effect_type: aura.aura_type,
                duration: aura.duration,
                magnitude: aura.magnitude,
                break_on_damage_threshold: aura.break_on_damage,
                accumulated_damage: 0.0,
                tick_interval: 0.0,
                time_until_next_tick: 0.0,
                caster: Some(entity),
                ability_name: def.name.to_string(),
                fear_direction: (0.0, 0.0),
                fear_direction_timer: 0.0,
            },
        });

        // Log CC
        if let Some((target_team, target_class, _, _)) = combatant_info.get(&target_entity) {
            let cc_type = format!("{:?}", aura.aura_type);
            let message = format!(
                "Team {} {} uses {} on Team {} {}",
                combatant.team,
                combatant.class.name(),
                def.name,
                target_team,
                target_class.name()
            );
            combat_log.log_crowd_control(
                combatant_id(combatant.team, combatant.class),
                combatant_id(*target_team, *target_class),
                cc_type,
                aura.duration,
                message,
            );
        }
    }

    info!(
        "Team {} {} uses {} on enemy!",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}

/// Try to use Sinister Strike.
/// Returns true if Sinister Strike was used.
#[allow(clippy::too_many_arguments)]
fn try_sinister_strike(
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target_entity: Entity,
    target_pos: Vec3,
    combatant_info: &HashMap<Entity, (u8, CharacterClass, f32, f32)>,
    instant_attacks: &mut Vec<(Entity, Entity, f32, u8, CharacterClass, AbilityType)>,
) -> bool {
    let ability = AbilityType::SinisterStrike;

    if !ability.can_cast(combatant, target_pos, my_pos) {
        return false;
    }

    let def = abilities.get_unchecked(&ability);

    // Execute Sinister Strike
    combatant.current_mana -= def.mana_cost;
    combatant.global_cooldown = GCD;

    // Calculate and queue damage
    let damage = combatant.calculate_ability_damage_config(def, game_rng);
    instant_attacks.push((
        entity,
        target_entity,
        damage,
        combatant.team,
        combatant.class,
        ability,
    ));

    // Log
    let caster_id = format!("Team {} {}", combatant.team, combatant.class.name());
    let target_id = combatant_info
        .get(&target_entity)
        .map(|(team, class, _, _)| format!("Team {} {}", team, class.name()));
    combat_log.log_ability_cast(
        caster_id,
        "Sinister Strike".to_string(),
        target_id,
        format!(
            "Team {} {} uses Sinister Strike",
            combatant.team,
            combatant.class.name()
        ),
    );

    info!(
        "Team {} {} uses {}!",
        combatant.team,
        combatant.class.name(),
        def.name
    );

    true
}
