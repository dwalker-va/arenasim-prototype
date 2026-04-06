//! Damage application, absorb shields, and interrupt processing.

use bevy::prelude::*;
use crate::combat::log::{CombatLog, CombatLogEventType};
use super::super::components::*;
use super::super::abilities::SpellSchool;
use super::super::ability_config::AbilityDefinitions;
use super::super::constants::DIVINE_SHIELD_DAMAGE_PENALTY;
use super::get_lockout_duration_reduction;

/// Roll a critical strike check. Returns true if the roll is a crit.
pub fn roll_crit(crit_chance: f32, rng: &mut GameRng) -> bool {
    rng.random_f32() < crit_chance
}

/// Apply damage to a combatant, accounting for absorb shields.
/// Returns (actual_damage_to_health, damage_absorbed).
///
/// If the target has an Absorb aura, damage is first subtracted from the shield.
/// Any remaining damage is applied to health. Depleted shields are removed.
///
/// # Panics (debug only)
/// Panics if damage is negative (damage should always be >= 0).
pub fn apply_damage_with_absorb(
    damage: f32,
    target: &mut Combatant,
    active_auras: Option<&mut ActiveAuras>,
    spell_school: SpellSchool,
) -> (f32, f32) {
    // Invariant: damage should never be negative
    debug_assert!(
        damage >= 0.0,
        "apply_damage_with_absorb: damage cannot be negative, got {}",
        damage
    );

    // Invariant: target health should be valid before we modify it
    debug_assert!(
        target.current_health >= 0.0,
        "apply_damage_with_absorb: target health already negative ({})",
        target.current_health
    );

    // Check for damage immunity (Divine Shield) — blocks all incoming damage
    if let Some(ref auras) = active_auras {
        if auras.auras.iter().any(|a| a.effect_type == AuraType::DamageImmunity) {
            return (0.0, 0.0);
        }
    }

    let mut remaining_damage = damage;
    let mut total_absorbed = 0.0;

    // Apply armor reduction for Physical damage
    if spell_school == SpellSchool::Physical && target.armor > 0.0 {
        let reduction = target.armor / (target.armor + 5500.0);
        remaining_damage *= 1.0 - reduction;
    }

    // Apply spell resistance for magical damage
    if spell_school != SpellSchool::Physical && spell_school != SpellSchool::None {
        let mut resistance = target.get_resistance(spell_school);

        // Sum SpellResistanceBuff auras matching this school
        if let Some(ref auras) = active_auras {
            for aura in auras.auras.iter() {
                if aura.effect_type == AuraType::SpellResistanceBuff
                    && aura.spell_school == Some(spell_school)
                {
                    resistance += aura.magnitude;
                }
            }
        }

        if resistance > 0.0 {
            let reduction = resistance / (resistance * 5.0 / 3.0 + 300.0);
            remaining_damage *= 1.0 - reduction;
        }
    }

    // Apply damage taken reduction (e.g., Devotion Aura)
    // Multiple reductions stack multiplicatively (two 10% reductions = 19% total)
    if let Some(ref auras) = active_auras {
        for aura in auras.auras.iter() {
            if aura.effect_type == AuraType::DamageTakenReduction && remaining_damage > 0.0 {
                remaining_damage *= 1.0 - aura.magnitude;
            }
        }
    }

    // Check for absorb shields and consume them
    if let Some(auras) = active_auras {
        for aura in auras.auras.iter_mut() {
            if aura.effect_type == AuraType::Absorb && remaining_damage > 0.0 {
                // Invariant: absorb shield magnitude should be positive
                debug_assert!(
                    aura.magnitude >= 0.0,
                    "apply_damage_with_absorb: absorb shield has negative magnitude ({})",
                    aura.magnitude
                );

                let absorb_amount = aura.magnitude.min(remaining_damage);
                aura.magnitude -= absorb_amount;
                remaining_damage -= absorb_amount;
                total_absorbed += absorb_amount;
            }
        }
        // Remove depleted absorb shields
        auras.auras.retain(|a| !(a.effect_type == AuraType::Absorb && a.magnitude <= 0.0));
    }

    // Apply remaining damage to health
    let actual_damage = remaining_damage.min(target.current_health);
    target.current_health = (target.current_health - remaining_damage).max(0.0);
    target.damage_taken += actual_damage;

    // Post-condition: health should still be valid
    debug_assert!(
        target.current_health >= 0.0,
        "apply_damage_with_absorb: health went negative after damage"
    );

    (actual_damage, total_absorbed)
}

/// Check if a combatant has an absorb shield active
pub fn has_absorb_shield(auras: Option<&ActiveAuras>) -> bool {
    auras.map_or(false, |a| a.auras.iter().any(|aura| aura.effect_type == AuraType::Absorb))
}

/// Check if a combatant has Weakened Soul (cannot receive Power Word: Shield)
pub fn has_weakened_soul(auras: Option<&ActiveAuras>) -> bool {
    auras.map_or(false, |a| a.auras.iter().any(|aura| aura.effect_type == AuraType::WeakenedSoul))
}

/// Get the physical damage reduction multiplier from DamageReduction auras on the attacker.
/// Used by Curse of Weakness to reduce outgoing physical damage by a percentage.
/// Returns the percentage reduction (0.2 = 20% less damage).
/// Multiple reductions stack additively (two 20% reductions = 40% total).
pub fn get_physical_damage_reduction(auras: Option<&ActiveAuras>) -> f32 {
    auras.map_or(0.0, |a| {
        a.auras
            .iter()
            .filter(|aura| aura.effect_type == AuraType::DamageReduction)
            .map(|aura| aura.magnitude)
            .sum()
    })
}

/// Check if a combatant has damage immunity (Divine Shield active)
pub fn has_damage_immunity(auras: Option<&ActiveAuras>) -> bool {
    auras.map_or(false, |a| a.auras.iter().any(|aura| aura.effect_type == AuraType::DamageImmunity))
}

/// Returns the outgoing damage multiplier for the caster.
/// If caster has DamageImmunity (Divine Shield), returns DIVINE_SHIELD_DAMAGE_PENALTY (0.5).
/// Otherwise returns 1.0 (no penalty).
pub fn get_divine_shield_damage_penalty(auras: Option<&ActiveAuras>) -> f32 {
    if has_damage_immunity(auras) {
        DIVINE_SHIELD_DAMAGE_PENALTY
    } else {
        1.0
    }
}

/// Process interrupt attempts: interrupt target's cast or channel and apply spell school lockout.
pub fn process_interrupts(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    abilities: Res<AbilityDefinitions>,
    interrupts: Query<(Entity, &InterruptPending)>,
    mut casting_targets: Query<(&mut CastingState, &Combatant), Without<ChannelingState>>,
    mut channeling_targets: Query<(&mut ChannelingState, &Combatant), Without<CastingState>>,
    combatants: Query<&Combatant>,
    pet_query: Query<&Pet>,
    celebration: Option<Res<VictoryCelebration>>,
    auras_query: Query<&ActiveAuras>,
) {
    // Don't process interrupts during victory celebration
    if celebration.is_some() {
        return;
    }

    for (interrupt_entity, interrupt) in interrupts.iter() {
        let mut interrupted = false;

        // Check if target is casting
        if let Ok((mut cast_state, target_combatant)) = casting_targets.get_mut(interrupt.target) {
            // Don't interrupt if already interrupted
            if !cast_state.interrupted {
                // Get the spell school of the interrupted spell
                let interrupted_ability_def = abilities.get_unchecked(&cast_state.ability);
                let interrupted_school = interrupted_ability_def.spell_school;
                let interrupted_spell_name = &interrupted_ability_def.name;

                // Mark cast as interrupted
                cast_state.interrupted = true;
                cast_state.interrupted_display_time = 0.5; // Show "INTERRUPTED" for 0.5 seconds

                // Mark the ability cast as interrupted in the combat log (for timeline visualization)
                let interrupted_caster_id = format!("Team {} {}", target_combatant.team, target_combatant.class.name());
                combat_log.mark_cast_interrupted(&interrupted_caster_id, interrupted_spell_name);

                // Check for lockout duration reduction (Concentration Aura)
                let lockout_reduction = get_lockout_duration_reduction(auras_query.get(interrupt.target).ok());

                // Apply lockout and log
                apply_interrupt_lockout(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    interrupt,
                    &combatants,
                    &pet_query,
                    target_combatant,
                    interrupted_school,
                    interrupted_spell_name,
                    lockout_reduction,
                );

                interrupted = true;
            }
        }

        // Check if target is channeling (if not already interrupted a cast)
        if !interrupted {
            if let Ok((mut channel_state, target_combatant)) = channeling_targets.get_mut(interrupt.target) {
                // Don't interrupt if already interrupted
                if !channel_state.interrupted {
                    // Get the spell school of the interrupted channel
                    let interrupted_ability_def = abilities.get_unchecked(&channel_state.ability);
                    let interrupted_school = interrupted_ability_def.spell_school;
                    let interrupted_spell_name = &interrupted_ability_def.name;

                    // Mark channel as interrupted
                    channel_state.interrupted = true;
                    channel_state.interrupted_display_time = 0.5; // Show "INTERRUPTED" for 0.5 seconds

                    // Mark the ability as interrupted in the combat log (for timeline visualization)
                    let interrupted_caster_id = format!("Team {} {}", target_combatant.team, target_combatant.class.name());
                    combat_log.mark_cast_interrupted(&interrupted_caster_id, interrupted_spell_name);

                    // Check for lockout duration reduction (Concentration Aura)
                    let lockout_reduction = get_lockout_duration_reduction(auras_query.get(interrupt.target).ok());

                    // Apply lockout and log
                    apply_interrupt_lockout(
                        &mut commands,
                        &mut combat_log,
                        &abilities,
                        interrupt,
                        &combatants,
                        &pet_query,
                        target_combatant,
                        interrupted_school,
                        interrupted_spell_name,
                        lockout_reduction,
                    );
                }
            }
        }

        // Despawn the interrupt entity
        commands.entity(interrupt_entity).despawn();
    }
}

/// Helper function to apply spell school lockout and log the interrupt.
fn apply_interrupt_lockout(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    interrupt: &InterruptPending,
    combatants: &Query<&Combatant>,
    pet_query: &Query<&Pet>,
    target_combatant: &Combatant,
    interrupted_school: SpellSchool,
    interrupted_spell_name: &str,
    lockout_reduction: f32,
) {
    // Get caster info for logging
    let caster_info = if let Ok(caster) = combatants.get(interrupt.caster) {
        let display_name = if let Ok(pet) = pet_query.get(interrupt.caster) {
            pet.pet_type.name().to_string()
        } else {
            caster.class.name().to_string()
        };
        (caster.team, display_name)
    } else {
        (0, "Unknown".to_string()) // Fallback
    };

    // Apply spell school lockout aura
    // Store the locked school as the magnitude (cast to f32)
    let locked_school_value = match interrupted_school {
        SpellSchool::Physical => 0.0,
        SpellSchool::Frost => 1.0,
        SpellSchool::Holy => 2.0,
        SpellSchool::Shadow => 3.0,
        SpellSchool::Arcane => 4.0,
        SpellSchool::Fire => 5.0,
        SpellSchool::Nature => 6.0,
        SpellSchool::None => 7.0,
    };

    // Apply lockout duration reduction (e.g., Concentration Aura reduces by 50%)
    let effective_lockout = interrupt.lockout_duration * (1.0 - lockout_reduction);

    commands.spawn(AuraPending {
        target: interrupt.target,
        aura: Aura {
            effect_type: AuraType::SpellSchoolLockout,
            duration: effective_lockout,
            magnitude: locked_school_value,
            break_on_damage_threshold: -1.0, // Never breaks on damage
            accumulated_damage: 0.0,
            tick_interval: 0.0,
            time_until_next_tick: 0.0,
            caster: Some(interrupt.caster),
            ability_name: abilities.get_unchecked(&interrupt.ability).name.clone(),
            fear_direction: (0.0, 0.0),
            fear_direction_timer: 0.0,
            spell_school: None, // Lockouts are not dispellable
        },
    });

    // Log the interrupt
    let school_name = match interrupted_school {
        SpellSchool::Physical => "Physical",
        SpellSchool::Frost => "Frost",
        SpellSchool::Holy => "Holy",
        SpellSchool::Shadow => "Shadow",
        SpellSchool::Arcane => "Arcane",
        SpellSchool::Fire => "Fire",
        SpellSchool::Nature => "Nature",
        SpellSchool::None => "None",
    };

    let message = format!(
        "Team {} {} interrupts Team {} {}'s {} - {} school locked for {:.1}s",
        caster_info.0,
        caster_info.1,
        target_combatant.team,
        target_combatant.class.name(),
        interrupted_spell_name,
        school_name,
        effective_lockout
    );
    combat_log.log(CombatLogEventType::AbilityUsed, message);

    info!(
        "Team {} {} interrupted! {} school locked for {:.1}s",
        target_combatant.team,
        target_combatant.class.name(),
        school_name,
        effective_lockout
    );
}
