//! Casting and channeling systems, resource regeneration, stealth visuals.

use bevy::prelude::*;
use bevy_egui::egui;
use crate::combat::log::{CombatLog, CombatLogEventType};
use super::super::match_config;
use super::super::components::*;
use super::super::abilities::AbilityType;
use super::super::abilities::SpellSchool;
use super::super::ability_config::AbilityDefinitions;
use super::super::constants::{CRIT_DAMAGE_MULTIPLIER, CRIT_HEALING_MULTIPLIER};
use super::super::utils::{spawn_speech_bubble, get_next_fct_offset, combatant_id};
use super::super::FCT_HEIGHT;
use super::damage::{roll_crit, apply_damage_with_absorb, get_physical_damage_reduction, get_divine_shield_damage_penalty};

/// Resource regeneration system: Regenerate mana for all combatants.
///
/// Each combatant with mana regeneration gains mana per second up to their max.
/// Also ticks down ability cooldowns over time.
pub fn regenerate_resources(
    time: Res<Time>,
    mut combatants: Query<&mut Combatant>,
) {
    let dt = time.delta_secs();

    for mut combatant in combatants.iter_mut() {
        if !combatant.is_alive() {
            continue;
        }

        // Regenerate mana/resources
        if combatant.mana_regen > 0.0 {
            combatant.current_mana = (combatant.current_mana + combatant.mana_regen * dt).min(combatant.max_mana);
        }

        // Tick down ability cooldowns
        let abilities_on_cooldown: Vec<AbilityType> = combatant.ability_cooldowns.keys().copied().collect();
        for ability in abilities_on_cooldown {
            if let Some(cooldown) = combatant.ability_cooldowns.get_mut(&ability) {
                *cooldown -= dt;
                if *cooldown <= 0.0 {
                    combatant.ability_cooldowns.remove(&ability);
                }
            }
        }

        // Tick down global cooldown
        if combatant.global_cooldown > 0.0 {
            combatant.global_cooldown -= dt;
            if combatant.global_cooldown < 0.0 {
                combatant.global_cooldown = 0.0;
            }
        }

        // Tick down kiting timer
        if combatant.kiting_timer > 0.0 {
            combatant.kiting_timer -= dt;
            if combatant.kiting_timer < 0.0 {
                combatant.kiting_timer = 0.0;
            }
        }
    }
}

/// Update visual appearance of stealthed combatants.
///
/// Makes stealthed Rogues semi-transparent (40% alpha) with a darker tint
/// to clearly indicate their stealth status. When they break stealth (e.g., by using Ambush),
/// they return to full opacity and original color.
pub fn update_stealth_visuals(
    combatants: Query<(&Combatant, &MeshMaterial3d<StandardMaterial>), Changed<Combatant>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (combatant, material_handle) in combatants.iter() {
        if let Some(material) = materials.get_mut(&material_handle.0) {
            let current_color = material.base_color.to_srgba();
            let current_alpha = current_color.alpha;

            if combatant.stealthed {
                // Only apply stealth effect if not already stealthed (alpha is 1.0)
                if current_alpha >= 0.9 {
                    // Semi-transparent with darker tint for stealth
                    let color = Color::srgba(
                        current_color.red * 0.6,
                        current_color.green * 0.6,
                        current_color.blue * 0.6,
                        0.4, // 40% opacity
                    );
                    material.base_color = color;
                }
            } else {
                // Only restore if currently stealthed (alpha is low)
                if current_alpha < 0.9 {
                    // Restore original color by reversing the darkening (divide by 0.6)
                    let color = Color::srgba(
                        (current_color.red / 0.6).min(1.0),
                        (current_color.green / 0.6).min(1.0),
                        (current_color.blue / 0.6).min(1.0),
                        1.0, // Full opacity
                    );
                    material.base_color = color;
                }
            }
        }
    }
}

/// Process casting: update cast timers and apply effects when casts complete.
///
/// When a cast completes:
/// 1. Consume mana
/// 2. Deal damage (for damage spells) or heal (for healing spells)
/// 3. Apply auras (if applicable)
/// 4. Spawn floating combat text (yellow for damage, green for healing)
/// 5. Log to combat log with position data
pub fn process_casting(
    time: Res<Time>,
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    abilities: Res<AbilityDefinitions>,
    mut game_rng: ResMut<GameRng>,
    mut combatants: Query<(Entity, &Transform, &mut Combatant, Option<&mut CastingState>, Option<&mut ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't complete casts during victory celebration
    if celebration.is_some() {
        return;
    }

    let dt = time.delta_secs();

    // Track completed casts
    let mut completed_casts = Vec::new();

    // First pass: update cast timers and collect completed casts
    for (caster_entity, caster_transform, mut caster, casting_state, caster_auras) in combatants.iter_mut() {
        let Some(mut casting) = casting_state else {
            continue;
        };

        if !caster.is_alive() {
            // Cancel cast if caster dies
            commands.entity(caster_entity).remove::<CastingState>();
            continue;
        }

        // WoW Mechanic: Stun, Fear, and Polymorph cancel casts in progress
        // (Root does NOT interrupt casting — only movement)
        let is_incapacitated = super::super::utils::is_incapacitated(caster_auras.as_deref());
        if is_incapacitated {
            let ability_def = abilities.get_unchecked(&casting.ability);
            let caster_id = format!("Team {} {}", caster.team, caster.class.name());
            combat_log.mark_cast_interrupted(&caster_id, &ability_def.name);
            combat_log.log(
                CombatLogEventType::CrowdControl,
                format!("{}'s {} interrupted by crowd control", caster_id, ability_def.name),
            );
            commands.entity(caster_entity).remove::<CastingState>();
            continue;
        }

        // Handle interrupted casts
        if casting.interrupted {
            // Tick down the interrupted display timer
            casting.interrupted_display_time -= dt;

            // Remove CastingState once display time expires
            if casting.interrupted_display_time <= 0.0 {
                commands.entity(caster_entity).remove::<CastingState>();
            }

            // Don't process interrupted casts
            continue;
        }

        // Tick down cast time
        casting.time_remaining -= dt;

        // Check if cast completed
        if casting.time_remaining <= 0.0 {
            let ability = casting.ability;
            let def = abilities.get_unchecked(&ability);
            let target_entity = casting.target;

            // Consume mana
            caster.current_mana -= def.mana_cost;

            // Pre-calculate damage/healing (using caster's stats)
            let mut ability_damage = caster.calculate_ability_damage_config(def, &mut game_rng);

            // Roll crit for damage (before physical damage reduction)
            let is_crit_damage = if def.is_damage() {
                let crit = roll_crit(caster.crit_chance, &mut game_rng);
                if crit { ability_damage *= CRIT_DAMAGE_MULTIPLIER; }
                crit
            } else {
                false
            };

            // Apply physical damage reduction for Physical abilities (Curse of Weakness: -20%)
            if def.spell_school == SpellSchool::Physical {
                let damage_reduction = get_physical_damage_reduction(caster_auras.as_deref());
                ability_damage = (ability_damage * (1.0 - damage_reduction)).max(0.0);
            }

            // Apply Divine Shield outgoing damage penalty (50%)
            let ds_penalty = get_divine_shield_damage_penalty(caster_auras.as_deref());
            ability_damage = (ability_damage * ds_penalty).max(0.0);

            let mut ability_healing = caster.calculate_ability_healing_config(def, &mut game_rng);

            // Roll crit for healing (before healing reduction)
            let is_crit_heal = if def.is_heal() {
                let crit = roll_crit(caster.crit_chance, &mut game_rng);
                if crit { ability_healing *= CRIT_HEALING_MULTIPLIER; }
                crit
            } else {
                false
            };

            // Store cast info for processing
            completed_casts.push((
                caster_entity,
                caster.team,
                caster.class,
                caster_transform.translation,
                ability_damage,
                ability_healing,
                ability,
                target_entity,
                is_crit_damage,
                is_crit_heal,
            ));

            // Remove casting state
            // Note: GCD was already triggered when the cast began, not here
            commands.entity(caster_entity).remove::<CastingState>();
        }
    }

    // Track damage_dealt updates for casters (to apply after processing all casts)
    let mut caster_damage_updates: Vec<(Entity, f32)> = Vec::new();
    // Track healing_done updates for healers (to apply after processing all casts)
    let mut caster_healing_updates: Vec<(Entity, f32)> = Vec::new();
    // Track ability cooldowns to apply (caster_entity, ability, cooldown_duration)
    let mut cooldown_updates: Vec<(Entity, AbilityType, f32)> = Vec::new();
    // Track casters who should have stealth broken (offensive abilities)
    let mut break_stealth: Vec<Entity> = Vec::new();

    // Process completed casts
    for (caster_entity, caster_team, caster_class, caster_pos, ability_damage, ability_healing, ability, target_entity, is_crit_damage, is_crit_heal) in completed_casts {
        let def = abilities.get_unchecked(&ability);

        // Get target
        let Some(target_entity) = target_entity else {
            continue;
        };

        // If this ability uses a projectile, spawn it and skip immediate effect application
        if let Some(projectile_speed) = def.projectile_speed {
            // Spawn projectile with Transform (required for move_projectiles to work in headless mode)
            // Visual mesh/material is added by spawn_projectile_visuals in graphical mode
            commands.spawn((
                Projectile {
                    caster: caster_entity,
                    target: target_entity,
                    ability,
                    speed: projectile_speed,
                    caster_team,
                    caster_class,
                },
                Transform::from_translation(caster_pos + Vec3::new(0.0, 1.5, 0.0)), // Spawn at chest height
                PlayMatchEntity,
            ));
            continue; // Skip immediate damage/healing - projectile will handle it on impact
        }

        // Check if this is self-targeting (e.g., priest healing themselves)
        let is_self_target = target_entity == caster_entity;

        // Get target combatant
        let Ok((_, target_transform, mut target, _, mut target_auras)) = combatants.get_mut(target_entity) else {
            continue;
        };

        if !target.is_alive() {
            continue;
        }

        let target_pos = target_transform.translation;
        let text_position = target_transform.translation + Vec3::new(0.0, FCT_HEIGHT, 0.0);

        // Handle damage spells
        if def.is_damage() {
            // Use pre-calculated damage (already includes stat scaling and DamageReduction)
            let damage = ability_damage;

            // Apply damage with absorb shield consideration
            let (actual_damage, absorbed) = apply_damage_with_absorb(
                damage,
                &mut target,
                target_auras.as_deref_mut(),
            );

            // Warriors generate Rage from taking damage (only on actual health damage)
            if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                let rage_gain = actual_damage * 0.15; // Gain 15% of damage taken as Rage
                target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
            }

            // Track damage for aura breaking
            commands.entity(target_entity).insert(DamageTakenThisFrame {
                amount: actual_damage,
            });

            // Break stealth on offensive ability use
            break_stealth.push(caster_entity);

            // Track damage dealt for caster (include absorbed - caster dealt it)
            let total_damage_dealt = actual_damage + absorbed;
            if is_self_target {
                // Self-damage: target IS caster, so update now
                target.damage_dealt += total_damage_dealt;
            } else {
                // Different target: collect for later update
                caster_damage_updates.push((caster_entity, total_damage_dealt));
            }

            // Spawn floating combat text (yellow for damage abilities)
            // Get deterministic offset based on pattern state
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: format!("{:.0}", actual_damage),
                    color: egui::Color32::from_rgb(255, 255, 0), // Yellow for abilities
                    lifetime: 1.5,
                    vertical_offset: offset_y,
                    is_crit: is_crit_damage,
                },
                PlayMatchEntity,
            ));

            // Spawn light blue floating combat text for absorbed damage
            if absorbed > 0.0 {
                let (absorb_offset_x, absorb_offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                    get_next_fct_offset(&mut fct_state)
                } else {
                    (0.0, 0.0)
                };
                commands.spawn((
                    FloatingCombatText {
                        world_position: text_position + Vec3::new(absorb_offset_x, absorb_offset_y, 0.0),
                        text: format!("{:.0} absorbed", absorbed),
                        color: egui::Color32::from_rgb(100, 180, 255), // Light blue
                        lifetime: 1.5,
                        vertical_offset: absorb_offset_y,
                        is_crit: false,
                    },
                    PlayMatchEntity,
                ));
            }

            // Spawn visual effect for Mind Blast (shadow impact)
            if ability == AbilityType::MindBlast {
                commands.spawn((
                    SpellImpactEffect {
                        position: target_pos,
                        lifetime: 0.5,
                        initial_lifetime: 0.5,
                        initial_scale: 0.5,
                        final_scale: 2.0,
                    },
                    PlayMatchEntity,
                ));
            }

            // Spawn flame particles for Immolate (fire rising effect)
            if ability == AbilityType::Immolate {
                // Spawn 8-12 flame particles around target
                let particle_count = 8 + (game_rng.random_f32() * 5.0) as i32;
                for _ in 0..particle_count {
                    // Randomize position slightly around target
                    let offset = Vec3::new(
                        (game_rng.random_f32() - 0.5) * 1.0,  // -0.5 to 0.5
                        game_rng.random_f32() * 0.5,          // 0 to 0.5 (start near ground)
                        (game_rng.random_f32() - 0.5) * 1.0,
                    );
                    let velocity = Vec3::new(
                        (game_rng.random_f32() - 0.5) * 0.5,  // Slight horizontal drift
                        2.0 + game_rng.random_f32() * 1.5,    // Upward: 2.0-3.5 units/sec
                        (game_rng.random_f32() - 0.5) * 0.5,
                    );
                    let lifetime = 0.6 + game_rng.random_f32() * 0.4;  // 0.6-1.0 sec
                    commands.spawn((
                        FlameParticle {
                            velocity,
                            lifetime,
                            initial_lifetime: lifetime,
                        },
                        Transform::from_translation(target_pos + offset),
                        PlayMatchEntity,
                    ));
                }
            }

            // Log the damage with structured data
            let is_killing_blow = !target.is_alive();
            let is_first_death = is_killing_blow && !target.is_dead;
            if is_first_death {
                target.is_dead = true;
            }
            let verb = if is_crit_damage { "CRITS" } else { "hits" };
            let message = if absorbed > 0.0 {
                format!(
                    "Team {} {}'s {} {} Team {} {} for {:.0} damage ({:.0} absorbed)",
                    caster_team,
                    caster_class.name(),
                    def.name,
                    verb,
                    target.team,
                    target.class.name(),
                    actual_damage,
                    absorbed
                )
            } else {
                format!(
                    "Team {} {}'s {} {} Team {} {} for {:.0} damage",
                    caster_team,
                    caster_class.name(),
                    def.name,
                    verb,
                    target.team,
                    target.class.name(),
                    actual_damage
                )
            };
            combat_log.log_damage(
                combatant_id(caster_team, caster_class),
                combatant_id(target.team, target.class),
                def.name.to_string(),
                total_damage_dealt, // Total damage including absorbed
                is_killing_blow,
                is_crit_damage,
                message,
            );

            if is_killing_blow {
                // Cancel any in-progress cast or channel so dead combatants can't finish spells
                commands.entity(target_entity).remove::<CastingState>();
                commands.entity(target_entity).remove::<ChannelingState>();

                let death_message = format!(
                    "Team {} {} has been eliminated",
                    target.team,
                    target.class.name()
                );
                combat_log.log_death(
                    combatant_id(target.team, target.class),
                    Some(combatant_id(caster_team, caster_class)),
                    death_message,
                );
            }
        }
        // Handle healing spells
        else if def.is_heal() {
            // Use pre-calculated healing (already includes stat scaling + crit)
            let mut healing = ability_healing;

            // Check for healing reduction auras
            if let Some(auras) = target_auras {
                for aura in &auras.auras {
                    if aura.effect_type == AuraType::HealingReduction {
                        // Magnitude is a multiplier (e.g., 0.65 = 35% reduction)
                        healing *= aura.magnitude;
                    }
                }
            }

            // Apply healing (don't overheal)
            let actual_healing = healing.min(target.max_health - target.current_health);
            target.current_health = (target.current_health + healing).min(target.max_health);

            // Track healing done for healer (update later to avoid double borrow)
            if is_self_target {
                // Self-healing: target IS caster, so update now
                target.healing_done += actual_healing;
            } else {
                // Different target: collect for later update
                caster_healing_updates.push((caster_entity, actual_healing));
            }

            // Spawn floating combat text (green for healing)
            // Get deterministic offset based on pattern state
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: format!("+{:.0}", actual_healing),
                    color: egui::Color32::from_rgb(100, 255, 100), // Green for healing
                    lifetime: 1.5,
                    vertical_offset: offset_y,
                    is_crit: is_crit_heal,
                },
                PlayMatchEntity,
            ));

            // Log the healing with structured data
            let verb = if is_crit_heal { "CRITICALLY heals" } else { "heals" };
            let message = format!(
                "Team {} {}'s {} {} Team {} {} for {:.0}",
                caster_team,
                caster_class.name(),
                def.name,
                verb,
                target.team,
                target.class.name(),
                actual_healing
            );
            combat_log.log_healing(
                combatant_id(caster_team, caster_class),
                combatant_id(target.team, target.class),
                def.name.to_string(),
                actual_healing,
                is_crit_heal,
                message,
            );

            // Spawn healing light column visual effect
            commands.spawn((
                HealingLightColumn {
                    target: target_entity,
                    healer_class: caster_class,
                    lifetime: 0.8,
                    initial_lifetime: 0.8,
                },
                PlayMatchEntity,
            ));
        }

        // Apply aura if applicable (store for later application)
        if let Some(aura) = def.applies_aura.as_ref() {
            if let Some(aura_pending) = AuraPending::from_ability(target_entity, caster_entity, def) {
                commands.spawn((aura_pending, PlayMatchEntity));
            }

            info!(
                "Queued {:?} aura for Team {} {} (magnitude: {}, duration: {}s)",
                aura.aura_type,
                target.team,
                target.class.name(),
                aura.magnitude,
                aura.duration
            );

            // Log CC application for all crowd control types
            match aura.aura_type {
                AuraType::Fear => {
                    spawn_speech_bubble(&mut commands, caster_entity, "Fear");
                    let message = format!(
                        "Team {} {}'s {} lands on Team {} {} ({:.1}s)",
                        caster_team,
                        caster_class.name(),
                        def.name,
                        target.team,
                        target.class.name(),
                        aura.duration
                    );
                    combat_log.log_crowd_control(
                        combatant_id(caster_team, caster_class),
                        combatant_id(target.team, target.class),
                        "Fear".to_string(),
                        aura.duration,
                        message,
                    );
                }
                AuraType::Root => {
                    let message = format!(
                        "Team {} {}'s {} roots Team {} {} ({:.1}s)",
                        caster_team,
                        caster_class.name(),
                        def.name,
                        target.team,
                        target.class.name(),
                        aura.duration
                    );
                    combat_log.log_crowd_control(
                        combatant_id(caster_team, caster_class),
                        combatant_id(target.team, target.class),
                        "Root".to_string(),
                        aura.duration,
                        message,
                    );
                }
                AuraType::Stun => {
                    let message = format!(
                        "Team {} {}'s {} stuns Team {} {} ({:.1}s)",
                        caster_team,
                        caster_class.name(),
                        def.name,
                        target.team,
                        target.class.name(),
                        aura.duration
                    );
                    combat_log.log_crowd_control(
                        combatant_id(caster_team, caster_class),
                        combatant_id(target.team, target.class),
                        "Stun".to_string(),
                        aura.duration,
                        message,
                    );
                }
                AuraType::Polymorph => {
                    let message = format!(
                        "Team {} {}'s {} polymorphs Team {} {} ({:.1}s)",
                        caster_team,
                        caster_class.name(),
                        def.name,
                        target.team,
                        target.class.name(),
                        aura.duration
                    );
                    combat_log.log_crowd_control(
                        combatant_id(caster_team, caster_class),
                        combatant_id(target.team, target.class),
                        "Polymorph".to_string(),
                        aura.duration,
                        message,
                    );
                }
                _ => {} // Non-CC auras don't need logging here
            }
        }

        // Track cooldown if ability has one
        if def.cooldown > 0.0 {
            cooldown_updates.push((caster_entity, ability, def.cooldown));
        }

        // Check for death (log if killed by non-damage abilities/auras)
        // Note: damage abilities already log death via is_first_death above
        if !target.is_alive() && !def.is_damage() && !target.is_dead {
            target.is_dead = true;

            // Cancel any in-progress cast or channel so dead combatants can't finish spells
            commands.entity(target_entity).remove::<CastingState>();
            commands.entity(target_entity).remove::<ChannelingState>();

            let message = format!(
                "Team {} {} has been eliminated",
                target.team,
                target.class.name()
            );
            combat_log.log_death(
                combatant_id(target.team, target.class),
                Some(combatant_id(caster_team, caster_class)),
                message,
            );
        }
    }

    // Apply collected caster damage updates
    for (caster_entity, damage) in caster_damage_updates {
        if let Ok((_, _, mut caster, _, _)) = combatants.get_mut(caster_entity) {
            caster.damage_dealt += damage;
        }
    }

    // Apply collected healer healing updates
    for (healer_entity, healing) in caster_healing_updates {
        if let Ok((_, _, mut healer, _, _)) = combatants.get_mut(healer_entity) {
            healer.healing_done += healing;
        }
    }

    // Apply collected ability cooldowns
    for (caster_entity, ability, cooldown) in cooldown_updates {
        if let Ok((_, _, mut caster, _, _)) = combatants.get_mut(caster_entity) {
            caster.ability_cooldowns.insert(ability, cooldown);
        }
    }

    // Break stealth for casters who used offensive abilities
    for caster_entity in break_stealth {
        if let Ok((_, _, mut caster, _, _)) = combatants.get_mut(caster_entity) {
            if caster.stealthed {
                caster.stealthed = false;
                info!(
                    "Team {} {} breaks stealth!",
                    caster.team,
                    caster.class.name()
                );
            }
        }
    }
}

/// Process channeling: update channel timers and apply tick effects.
///
/// When a channel tick occurs:
/// 1. Deal damage to target (with absorb shield handling)
/// 2. Heal caster (reduced by healing reduction auras)
/// 3. Spawn floating combat text (yellow for damage, green for heal)
/// 4. Log to combat log
///
/// Channel ends when duration expires or either caster/target dies.
pub fn process_channeling(
    time: Res<Time>,
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    abilities: Res<AbilityDefinitions>,
    mut combatants: Query<(Entity, &Transform, &mut Combatant, Option<&mut ChannelingState>, Option<&mut ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
    celebration: Option<Res<VictoryCelebration>>,
) {
    // Don't process channels during victory celebration
    if celebration.is_some() {
        return;
    }

    let dt = time.delta_secs();

    // Track updates to apply after the loop
    let mut remove_channel: Vec<Entity> = Vec::new();
    let mut caster_healing_updates: Vec<(Entity, f32)> = Vec::new();
    // (caster_entity, target_entity, damage, caster_team, caster_class)
    let mut damage_to_apply: Vec<(Entity, Entity, f32, u8, match_config::CharacterClass)> = Vec::new();

    // Build a snapshot of positions and health for lookups
    let positions: std::collections::HashMap<Entity, Vec3> = combatants
        .iter()
        .map(|(entity, transform, _, _, _)| (entity, transform.translation))
        .collect();
    let health_info: std::collections::HashMap<Entity, (bool, u8, match_config::CharacterClass)> = combatants
        .iter()
        .map(|(entity, _, combatant, _, _)| (entity, (combatant.is_alive(), combatant.team, combatant.class)))
        .collect();
    // Snapshot target immunity status for Drain Life healing suppression
    let immunity_info: std::collections::HashSet<Entity> = combatants
        .iter()
        .filter(|(_, _, _, _, auras)| {
            auras.as_ref().map_or(false, |a| a.auras.iter().any(|aura| aura.effect_type == AuraType::DamageImmunity))
        })
        .map(|(entity, _, _, _, _)| entity)
        .collect();

    for (caster_entity, _caster_transform, caster, channeling_state, caster_auras) in combatants.iter_mut() {
        let Some(mut channeling) = channeling_state else {
            continue;
        };

        // Handle interrupted state display
        if channeling.interrupted {
            channeling.interrupted_display_time -= dt;
            if channeling.interrupted_display_time <= 0.0 {
                remove_channel.push(caster_entity);
            }
            continue;
        }

        // Check if caster died
        if !caster.is_alive() {
            remove_channel.push(caster_entity);
            continue;
        }

        // WoW Mechanic: Stun, Fear, and Polymorph cancel channels in progress
        // (Root does NOT interrupt channeling — only movement)
        let is_incapacitated = super::super::utils::is_incapacitated(caster_auras.as_deref());
        if is_incapacitated {
            let ability_def = abilities.get_unchecked(&channeling.ability);
            let caster_id = format!("Team {} {}", caster.team, caster.class.name());
            combat_log.mark_cast_interrupted(&caster_id, &ability_def.name);
            combat_log.log(
                CombatLogEventType::CrowdControl,
                format!("{}'s {} interrupted by crowd control", caster_id, ability_def.name),
            );
            remove_channel.push(caster_entity);
            continue;
        }

        // Check if target died or no longer exists
        let target_alive = health_info
            .get(&channeling.target)
            .map(|(alive, _, _)| *alive)
            .unwrap_or(false);
        if !target_alive {
            remove_channel.push(caster_entity);
            continue;
        }

        // Tick down timers
        channeling.duration_remaining -= dt;
        channeling.time_until_next_tick -= dt;

        // Process tick if ready
        if channeling.time_until_next_tick <= 0.0 {
            let ability_def = abilities.get_unchecked(&channeling.ability);

            // Calculate damage (flat damage per tick for Drain Life)
            let damage = ability_def.damage_base_min;

            // Track damage to apply later (includes target entity and caster info for death logging)
            damage_to_apply.push((caster_entity, channeling.target, damage, caster.team, caster.class));

            // Track healing for caster (Drain Life heals 0 if target has DamageImmunity)
            let healing = ability_def.channel_healing_per_tick;
            let target_immune = immunity_info.contains(&channeling.target);
            if healing > 0.0 && !target_immune {
                caster_healing_updates.push((caster_entity, healing));
            }

            // Log the tick
            if let Some(&(_, target_team, target_class)) = health_info.get(&channeling.target) {
                let damage_message = format!(
                    "Team {} {}'s {} ticks on Team {} {} for {:.0} damage",
                    caster.team,
                    caster.class.name(),
                    ability_def.name,
                    target_team,
                    target_class.name(),
                    damage
                );
                combat_log.log_damage(
                    combatant_id(caster.team, caster.class),
                    combatant_id(target_team, target_class),
                    format!("{} (tick)", ability_def.name),
                    damage,
                    false, // Not a killing blow check here - will be handled when applying damage
                    false, // is_crit - channel ticks never crit
                    damage_message,
                );

                if healing > 0.0 {
                    let heal_message = format!(
                        "Team {} {}'s {} heals for {:.0}",
                        caster.team,
                        caster.class.name(),
                        ability_def.name,
                        healing
                    );
                    combat_log.log_healing(
                        combatant_id(caster.team, caster.class),
                        combatant_id(caster.team, caster.class),
                        format!("{} (tick)", ability_def.name),
                        healing,
                        false, // is_crit - channel ticks never crit
                        heal_message,
                    );
                }
            }

            // Spawn floating combat text for damage on target
            if let Some(&target_pos) = positions.get(&channeling.target) {
                let text_position = target_pos + Vec3::new(0.0, FCT_HEIGHT, 0.0);
                let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(channeling.target) {
                    get_next_fct_offset(&mut fct_state)
                } else {
                    (0.0, 0.0)
                };
                commands.spawn((
                    FloatingCombatText {
                        world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                        text: format!("{:.0}", damage),
                        color: egui::Color32::from_rgb(255, 255, 0), // Yellow for ability damage
                        lifetime: 1.5,
                        vertical_offset: offset_y,
                        is_crit: false,
                    },
                    PlayMatchEntity,
                ));
            }

            // Reset tick timer
            channeling.time_until_next_tick = channeling.tick_interval;
            channeling.ticks_applied += 1;
        }

        // Check if channel duration expired
        if channeling.duration_remaining <= 0.0 {
            remove_channel.push(caster_entity);

            info!(
                "Team {} {} completed {} channel ({} ticks)",
                caster.team,
                caster.class.name(),
                abilities.get_unchecked(&channeling.ability).name,
                channeling.ticks_applied
            );
        }
    }

    // Apply damage to targets and update caster stats
    for (caster_entity, target_entity, damage, caster_team, caster_class) in damage_to_apply {
        // Apply damage to target
        if let Ok((_, target_transform, mut target, _, mut target_auras)) = combatants.get_mut(target_entity) {
            if target.is_alive() {
                let (actual_damage, absorbed) = apply_damage_with_absorb(
                    damage,
                    &mut target,
                    target_auras.as_deref_mut(),
                );

                // Track damage for aura breaking
                if actual_damage > 0.0 {
                    commands.entity(target_entity).insert(DamageTakenThisFrame {
                        amount: actual_damage,
                    });
                }

                // Spawn absorbed text if any
                if absorbed > 0.0 {
                    let text_position = target_transform.translation + Vec3::new(0.0, FCT_HEIGHT, 0.0);
                    let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                        get_next_fct_offset(&mut fct_state)
                    } else {
                        (0.0, 0.0)
                    };
                    commands.spawn((
                        FloatingCombatText {
                            world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                            text: format!("{:.0} absorbed", absorbed),
                            color: egui::Color32::from_rgb(100, 180, 255), // Light blue
                            lifetime: 1.5,
                            vertical_offset: offset_y,
                            is_crit: false,
                        },
                        PlayMatchEntity,
                    ));
                }

                // Warriors generate Rage from taking damage
                if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15;
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }

                // Check for killing blow
                if !target.is_alive() && !target.is_dead {
                    target.is_dead = true;

                    // Cancel any in-progress cast or channel so dead combatants can't finish spells
                    commands.entity(target_entity).remove::<CastingState>();
                    commands.entity(target_entity).remove::<ChannelingState>();

                    let death_message = format!(
                        "Team {} {} has been eliminated",
                        target.team,
                        target.class.name()
                    );
                    combat_log.log_death(
                        combatant_id(target.team, target.class),
                        Some(combatant_id(caster_team, caster_class)),
                        death_message,
                    );
                }
            }
        }

        // Update caster damage dealt stats
        if let Ok((_, _, mut caster, _, _)) = combatants.get_mut(caster_entity) {
            caster.damage_dealt += damage;
        }
    }

    // Apply healing to casters and spawn healing FCT
    for (caster_entity, healing) in caster_healing_updates {
        if let Ok((_, caster_transform, mut caster, _, caster_auras)) = combatants.get_mut(caster_entity) {
            let mut actual_healing = healing;

            // Check for healing reduction auras
            if let Some(auras) = caster_auras {
                for aura in &auras.auras {
                    if aura.effect_type == AuraType::HealingReduction {
                        actual_healing *= aura.magnitude;
                    }
                }
            }

            // Apply healing
            let effective_healing = actual_healing.min(caster.max_health - caster.current_health);
            caster.current_health = (caster.current_health + actual_healing).min(caster.max_health);
            caster.healing_done += effective_healing;

            // Spawn floating combat text for healing
            let text_position = caster_transform.translation + Vec3::new(0.0, FCT_HEIGHT, 0.0);
            let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(caster_entity) {
                get_next_fct_offset(&mut fct_state)
            } else {
                (0.0, 0.0)
            };
            commands.spawn((
                FloatingCombatText {
                    world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                    text: format!("+{:.0}", effective_healing),
                    color: egui::Color32::from_rgb(100, 255, 100), // Green for healing
                    lifetime: 1.5,
                    vertical_offset: offset_y,
                    is_crit: false,
                },
                PlayMatchEntity,
            ));
        }
    }

    // Remove completed/interrupted channels
    for entity in remove_channel {
        commands.entity(entity).remove::<ChannelingState>();
    }
}
