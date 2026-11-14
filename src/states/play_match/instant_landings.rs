//! Where a frame's queued instant effects land: the strikes (Mortal Strike,
//! Sinister Strike, Ambush, Crusader Strike, Hemorrhage), the area damage (Frost Nova,
//! Holy Nova) and the area heals (Holy Nova) the class AIs decided in
//! `combat_ai::decide_abilities`.
//!
//! **Every combatant, whatever it is doing.** `decide_abilities` dispatches
//! only the units that are free to act, so its query leaves out every unit
//! mid-cast or mid-channel. A strike landed through that query on a target
//! that was casting found no target: no damage, no log line, no proc, while
//! its attacker had already paid the resource and the cooldown (AS-488,
//! about one Mortal or Sinister Strike in five). The landing is therefore its
//! own system, the next in the chain, over an unfiltered query.
//! `tests/instant_lands_on_casters.rs` holds every damaging instant in the
//! ability table to a landing on a casting and on a channeling target.
//!
//! Order is the dispatch's: the strikes, then the area damage, then the area
//! heals, each in the order queued. Every strike queued lands, even one whose
//! attacker died earlier in the frame (dying-blow semantics); an area effect
//! whose caster died to a strike this frame does not.

use bevy::prelude::*;

use super::class_ai::combat_snapshot::CombatSnapshot;
use super::class_ai::{QueuedAoeDamage, QueuedAoeHeal, QueuedInstantAttack};
use super::components::*;
use super::utils::{combat_log_id_for, combatant_id, get_next_fct_offset};
use crate::combat::log::CombatLog;

/// A strike to land, with what its attacker brought to it as it decided.
#[derive(Clone, Copy)]
pub struct PendingStrike {
    /// The strike, its damage after Divine Shield's outgoing penalty.
    pub attack: QueuedInstantAttack,
    /// The attacker's damage-dealt multiplier in the strike's school.
    pub dealt: f32,
}

/// One victim's area hit to land, with its caster's damage-dealt multiplier.
#[derive(Clone, Copy)]
pub struct PendingAreaHit {
    pub hit: QueuedAoeDamage,
    pub dealt: f32,
}

/// The instant effects one frame's dispatch queued, for
/// [`land_instant_effects`]. Empty between frames.
#[derive(Resource, Default)]
pub struct InstantLandings {
    pub strikes: Vec<PendingStrike>,
    pub area_damage: Vec<PendingAreaHit>,
    pub area_heals: Vec<QueuedAoeHeal>,
}

impl InstantLandings {
    /// Queue a frame's dispatch: each strike and area hit with what its
    /// attacker brought to it in `snapshot`, the world it decided from.
    pub fn queue(
        &mut self,
        strikes: Vec<QueuedInstantAttack>,
        area_damage: Vec<QueuedAoeDamage>,
        area_heals: Vec<QueuedAoeHeal>,
        snapshot: &CombatSnapshot,
    ) {
        let attacker_auras =
            |e: Entity| -> &[Aura] { snapshot.active_auras.get(&e).map_or(&[], Vec::as_slice) };
        for attack in strikes {
            // The attacker's own book: it was in this frame's snapshot to
            // queue the attack at all.
            let school = snapshot
                .combatants
                .get(&attack.attacker)
                .expect("an attacker queued its attack from this frame's snapshot")
                .book
                .get_unchecked(&attack.ability)
                .spell_school;
            let auras = attacker_auras(attack.attacker);
            // Divine Shield's outgoing damage penalty (50%).
            let ds_penalty = if auras
                .iter()
                .any(|a| a.effect_type == AuraType::DamageImmunity)
            {
                super::constants::DIVINE_SHIELD_DAMAGE_PENALTY
            } else {
                1.0
            };
            self.strikes.push(PendingStrike {
                attack: QueuedInstantAttack {
                    damage: (attack.damage * ds_penalty).max(0.0),
                    ..attack
                },
                dealt: super::combat_core::DealtMods::from_auras(auras).multiplier(school),
            });
        }
        for hit in area_damage {
            self.area_damage.push(PendingAreaHit {
                dealt: super::combat_core::DealtMods::from_auras(attacker_auras(hit.caster))
                    .multiplier(hit.school),
                hit,
            });
        }
        self.area_heals.extend(area_heals);
    }
}

/// Land the frame's queued instant effects (see the module docs). Runs right
/// after `decide_abilities`, under the same condition.
pub fn land_instant_effects(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut landings: ResMut<InstantLandings>,
    // Every combatant: no `Without<CastingState>` or `Without<ChannelingState>`
    // here, or a strike on a caster vanishes (AS-488).
    mut combatants: Query<(Entity, &mut Combatant, &Transform, Option<&mut ActiveAuras>)>,
    mut fct_states: Query<&mut FloatingTextState>,
    pet_query: Query<&Pet>,
    dampening: Option<Res<ArenaDampening>>,
) {
    // What the frame's hits set off on other units (transfers, reflects,
    // saves), spawned once they have all landed.
    let mut follow_ups = super::combat_core::DamageFollowUps::default();
    let InstantLandings {
        strikes,
        area_damage,
        area_heals,
    } = std::mem::take(&mut *landings);

    // Queued instant strikes (Ambush, Sinister Strike, Mortal Strike,
    // Crusader Strike, Hemorrhage).
    for PendingStrike { attack, dealt } in strikes {
        let QueuedInstantAttack {
            attacker: attacker_entity,
            target: target_entity,
            damage,
            attacker_team,
            attacker_slot,
            attacker_class,
            ability,
            is_crit,
        } = attack;
        // The attacker's own book.
        let (ability_name, ability_spell_school) = {
            let (_, attacker, _, _) = combatants
                .get(attacker_entity)
                .expect("a queued strike's attacker is a combatant");
            let def = attacker.book().get_unchecked(&ability);
            (def.name.clone(), def.spell_school)
        };
        let mut actual_damage = 0.0;
        let mut absorbed_damage = 0.0;
        let mut landed = false;

        if let Ok((_, mut target, target_transform, mut target_auras)) =
            combatants.get_mut(target_entity)
        {
            if target.is_alive() {
                // Apply damage with absorb shield consideration
                let hit = super::combat_core::Hit::new(
                    damage,
                    ability_spell_school,
                    &ability_name,
                    Some(attacker_entity),
                    None,
                    target_entity,
                )
                .with_dealt(dealt)
                .melee_if(ability.is_melee_attack());
                let (dmg, absorbed) = super::combat_core::apply_damage_with_absorb(
                    hit,
                    &mut target,
                    target_auras.as_deref_mut(),
                    &mut follow_ups,
                    &mut combat_log,
                );
                actual_damage = dmg;
                absorbed_damage = absorbed;
                // Pet-aware ids for structured fields + message/trace text.
                let target_id = combat_log_id_for(&target, pet_query.get(target_entity).ok());
                let attacker_id = combatant_id(attacker_team, attacker_slot, attacker_class);

                // Warriors generate Rage from taking damage (only on actual health damage)
                if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15;
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }

                // Track damage for aura breaking
                commands.entity(target_entity).insert(DamageTakenThisFrame {
                    amount: actual_damage,
                });

                info!(
                    "{}'s {} hits {} for {:.0} damage!",
                    attacker_id, ability_name, target_id, actual_damage
                );

                // Cosmetic outcome marker for the graphical signature-animation
                // router (`rendering/effects/instant_ability.rs`). Spawned here,
                // inside the landed-hit gate, so a same-frame death never
                // telegraphs a phantom strike — the same rule `AutoAttackSwing`
                // follows. Ability-agnostic and byte-neutral: no `game_rng`
                // draw, and headless spawns it and never reads it (see
                // `cosmetic-marker-cross-mode-spawn-parity.md`).
                commands.spawn((
                    InstantAbilityFired {
                        caster: attacker_entity,
                        target: Some(target_entity),
                        ability,
                        is_crit,
                    },
                    PlayMatchEntity,
                ));

                // Spawn floating combat text (yellow for abilities)
                let text_position =
                    target_transform.translation + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
                // Get deterministic offset based on pattern state
                let (offset_x, offset_y) =
                    if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                        get_next_fct_offset(&mut fct_state)
                    } else {
                        (0.0, 0.0)
                    };
                commands.spawn((
                    FloatingCombatText {
                        world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                        text: format!("{:.0}", actual_damage),
                        color: crate::ui::theme::fct::DAMAGE,
                        lifetime: 1.5,
                        vertical_offset: offset_y,
                        is_crit,
                    },
                    PlayMatchEntity,
                ));

                // Spawn light blue floating combat text for absorbed damage
                if absorbed > 0.0 {
                    let (absorb_offset_x, absorb_offset_y) =
                        if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                            get_next_fct_offset(&mut fct_state)
                        } else {
                            (0.0, 0.0)
                        };
                    commands.spawn((
                        FloatingCombatText {
                            world_position: text_position
                                + Vec3::new(absorb_offset_x, absorb_offset_y, 0.0),
                            text: format!("{:.0} absorbed", absorbed),
                            color: crate::ui::theme::fct::ABSORB,
                            lifetime: 1.5,
                            vertical_offset: absorb_offset_y,
                            is_crit: false,
                        },
                        PlayMatchEntity,
                    ));
                }

                // Log the instant attack with structured data
                let is_killing_blow = !target.is_alive();
                let is_first_death = is_killing_blow && !target.is_dead;
                if is_first_death {
                    target.is_dead = true;
                }
                let verb = if is_crit { "CRITS" } else { "hits" };
                let message = if absorbed > 0.0 {
                    format!(
                        "{}'s {} {} {} for {:.0} damage ({:.0} absorbed)",
                        attacker_id, ability_name, verb, target_id, actual_damage, absorbed
                    )
                } else {
                    format!(
                        "{}'s {} {} {} for {:.0} damage",
                        attacker_id, ability_name, verb, target_id, actual_damage
                    )
                };
                combat_log.log_damage(
                    attacker_id.clone(),
                    target_id.clone(),
                    ability_name.to_string(),
                    super::combat_core::credited_damage(actual_damage, absorbed),
                    is_killing_blow,
                    is_crit,
                    message,
                );

                // Log death with killer tracking (only on first death to prevent duplicates)
                if is_first_death {
                    // Cancel any in-progress cast or channel so dead combatants can't finish spells
                    commands.entity(target_entity).remove::<CastingState>();
                    commands.entity(target_entity).remove::<ChannelingState>();

                    let death_message = format!("{} has been eliminated", target_id);
                    combat_log.log_death(
                        target_id.clone(),
                        Some(attacker_id.clone()),
                        death_message,
                    );
                }

                // A crit landing on the target is its `CritTaken`.
                if is_crit {
                    super::talent_procs::push_for(
                        &mut commands,
                        &target.talent_procs,
                        super::talent_procs::ProcEvent::new(
                            target_entity,
                            super::talent_procs::TalentTrigger::CritTaken,
                        )
                        .crit(true)
                        .amount(actual_damage)
                        .other(attacker_entity),
                    );
                }
                landed = true;
            }
        }

        // Update attacker's damage dealt
        if let Ok((_, mut attacker, _, _)) = combatants.get_mut(attacker_entity) {
            super::combat_core::credit_damage_dealt(&mut attacker, actual_damage, absorbed_damage);
            // The strike's `SpellHit` for its attacker.
            if landed {
                super::talent_procs::push_for(
                    &mut commands,
                    &attacker.talent_procs,
                    super::talent_procs::ProcEvent::new(
                        attacker_entity,
                        super::talent_procs::TalentTrigger::SpellHit,
                    )
                    .ability(ability)
                    .crit(is_crit)
                    .amount(actual_damage)
                    .other(target_entity),
                );
            }
        }
    }

    // Process queued area damage (Frost Nova, Holy Nova)
    for PendingAreaHit { hit: aoe, dealt } in area_damage {
        let QueuedAoeDamage {
            caster: caster_entity,
            target: target_entity,
            damage,
            caster_team,
            caster_slot,
            caster_class,
            target_pos: _target_pos,
            is_crit,
            ability,
            school,
        } = aoe;
        let mut actual_damage = 0.0;
        let mut absorbed_damage = 0.0;
        let mut nova_landed = false;

        // Skip if caster died from an instant attack earlier this frame (or
        // entity is gone). The effect's name is read from its caster's book.
        let name = match combatants.get(caster_entity) {
            Ok((_, caster, _, _)) if caster.is_alive() => caster
                .book()
                .get(&ability)
                .map_or_else(|| format!("{ability:?}"), |c| c.name.clone()),
            _ => continue,
        };

        if let Ok((_, mut target, target_transform, mut target_auras)) =
            combatants.get_mut(target_entity)
        {
            if target.is_alive() {
                // Apply damage with absorb shield consideration, in the
                // effect's own school.
                let hit = super::combat_core::Hit::new(
                    damage,
                    school,
                    &name,
                    Some(caster_entity),
                    None,
                    target_entity,
                )
                .with_dealt(dealt);
                let (dmg, absorbed) = super::combat_core::apply_damage_with_absorb(
                    hit,
                    &mut target,
                    target_auras.as_deref_mut(),
                    &mut follow_ups,
                    &mut combat_log,
                );
                actual_damage = dmg;
                absorbed_damage = absorbed;
                nova_landed = true;
                // Pet-aware target id (structured fields + message text).
                let target_id = combat_log_id_for(&target, pet_query.get(target_entity).ok());

                // Warriors generate Rage from taking damage (only on actual health damage)
                if actual_damage > 0.0 && target.resource_type == ResourceType::Rage {
                    let rage_gain = actual_damage * 0.15;
                    target.current_mana = (target.current_mana + rage_gain).min(target.max_mana);
                }

                // Track damage for aura breaking
                commands.entity(target_entity).insert(DamageTakenThisFrame {
                    amount: actual_damage,
                });

                // Spawn floating combat text (yellow for abilities)
                let text_position =
                    target_transform.translation + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
                // Get deterministic offset based on pattern state
                let (offset_x, offset_y) =
                    if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                        get_next_fct_offset(&mut fct_state)
                    } else {
                        (0.0, 0.0)
                    };
                commands.spawn((
                    FloatingCombatText {
                        world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                        text: format!("{:.0}", actual_damage),
                        color: crate::ui::theme::fct::DAMAGE,
                        lifetime: 1.5,
                        vertical_offset: offset_y,
                        is_crit,
                    },
                    PlayMatchEntity,
                ));

                // Spawn light blue floating combat text for absorbed damage
                if absorbed > 0.0 {
                    let (absorb_offset_x, absorb_offset_y) =
                        if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
                            get_next_fct_offset(&mut fct_state)
                        } else {
                            (0.0, 0.0)
                        };
                    commands.spawn((
                        FloatingCombatText {
                            world_position: text_position
                                + Vec3::new(absorb_offset_x, absorb_offset_y, 0.0),
                            text: format!("{:.0} absorbed", absorbed),
                            color: crate::ui::theme::fct::ABSORB,
                            lifetime: 1.5,
                            vertical_offset: absorb_offset_y,
                            is_crit: false,
                        },
                        PlayMatchEntity,
                    ));
                }

                // Log the area damage with structured data
                let is_killing_blow = !target.is_alive();
                let is_first_death = is_killing_blow && !target.is_dead;
                if is_first_death {
                    target.is_dead = true;
                }
                let caster_id = combatant_id(caster_team, caster_slot, caster_class);
                let verb = if is_crit { "CRITS" } else { "hits" };
                let message = if absorbed > 0.0 {
                    format!(
                        "{}'s {} {} {} for {:.0} damage ({:.0} absorbed)",
                        caster_id, name, verb, target_id, actual_damage, absorbed
                    )
                } else {
                    format!(
                        "{}'s {} {} {} for {:.0} damage",
                        caster_id, name, verb, target_id, actual_damage
                    )
                };
                combat_log.log_damage(
                    caster_id.clone(),
                    target_id.clone(),
                    name.clone(),
                    super::combat_core::credited_damage(actual_damage, absorbed),
                    is_killing_blow,
                    is_crit,
                    message,
                );

                // Log death with killer tracking (only on first death to prevent duplicates)
                if is_first_death {
                    // Cancel any in-progress cast or channel so dead combatants can't finish spells
                    commands.entity(target_entity).remove::<CastingState>();
                    commands.entity(target_entity).remove::<ChannelingState>();

                    let death_message = format!("{} has been eliminated", target_id);
                    combat_log.log_death(target_id.clone(), Some(caster_id.clone()), death_message);
                }
            }
        }

        // Update caster's damage dealt
        if let Ok((_, mut caster, _, _)) = combatants.get_mut(caster_entity) {
            super::combat_core::credit_damage_dealt(&mut caster, actual_damage, absorbed_damage);
            // The nova's hit on this target is its caster's `SpellHit`.
            if nova_landed {
                super::talent_procs::push_for(
                    &mut commands,
                    &caster.talent_procs,
                    super::talent_procs::ProcEvent::new(
                        caster_entity,
                        super::talent_procs::TalentTrigger::SpellHit,
                    )
                    .ability(ability)
                    .crit(is_crit)
                    .amount(actual_damage)
                    .other(target_entity),
                );
            }
        }
    }

    // Process queued area healing (Holy Nova), through the one heal site.
    let no_dampening = ArenaDampening::default();
    let dampening = dampening.as_deref().unwrap_or(&no_dampening);
    for aoe in area_heals {
        let QueuedAoeHeal {
            caster: caster_entity,
            target: target_entity,
            heal,
            is_crit,
            caster_team,
            caster_slot,
            caster_class,
            ability,
        } = aoe;
        let name = match combatants.get(caster_entity) {
            Ok((_, caster, _, _)) if caster.is_alive() => caster
                .book()
                .get(&ability)
                .map_or_else(|| format!("{ability:?}"), |c| c.name.clone()),
            _ => continue,
        };
        let Ok((_, mut target, target_transform, target_auras)) = combatants.get_mut(target_entity)
        else {
            continue;
        };
        if !target.is_alive() {
            continue;
        }
        let healed = super::combat_core::apply_healing(
            &mut commands,
            target_entity,
            &mut target,
            target_auras.as_deref(),
            dampening,
            heal,
        );
        let target_id = combat_log_id_for(&target, pet_query.get(target_entity).ok());
        let text_position = target_transform.translation + Vec3::new(0.0, super::FCT_HEIGHT, 0.0);
        let (offset_x, offset_y) = if let Ok(mut fct_state) = fct_states.get_mut(target_entity) {
            get_next_fct_offset(&mut fct_state)
        } else {
            (0.0, 0.0)
        };
        commands.spawn((
            FloatingCombatText {
                world_position: text_position + Vec3::new(offset_x, offset_y, 0.0),
                text: format!("+{:.0}", healed),
                color: crate::ui::theme::fct::HEAL,
                lifetime: 1.5,
                vertical_offset: offset_y,
                is_crit,
            },
            PlayMatchEntity,
        ));
        let caster_id = combatant_id(caster_team, caster_slot, caster_class);
        let verb = if is_crit { "CRITICALLY heals" } else { "heals" };
        let message = format!(
            "{}'s {} {} {} for {:.0}",
            caster_id, name, verb, target_id, healed
        );
        combat_log.log_healing(caster_id, target_id, name, healed, is_crit, message);
        if let Ok((_, mut caster, _, _)) = combatants.get_mut(caster_entity) {
            super::combat_core::credit_healing_done(&mut caster, healed);
        }
        // A critical heal is its caster's `HealCrit`.
        if is_crit {
            if let Ok((_, caster, _, _)) = combatants.get(caster_entity) {
                super::talent_procs::push_for(
                    &mut commands,
                    &caster.talent_procs,
                    super::talent_procs::ProcEvent::new(
                        caster_entity,
                        super::talent_procs::TalentTrigger::HealCrit,
                    )
                    .ability(ability)
                    .crit(true)
                    .amount(healed)
                    .other(target_entity),
                );
            }
        }
    }
    follow_ups.spawn(&mut commands);
}
