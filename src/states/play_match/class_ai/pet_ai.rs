//! Pet AI System
//!
//! Handles AI decisions for pet entities (Felhunter, Spider, Boar, Bird).
//! Runs separately from class AI - pets are skipped in the main dispatch loop
//! and processed here instead.

use bevy::prelude::*;

use super::super::utils::pet_combatant_id;
use super::CombatContext;
use crate::combat::log::CombatLog;
use crate::states::match_config::CharacterClass;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::*;
use crate::states::play_match::decision_trace::{
    ActorView, DecisionEventBuilder, DecisionTrace, RejectionReason, TargetView,
};
use crate::states::play_match::map_geometry::has_line_of_sight;

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
        (
            Entity,
            &mut Combatant,
            &Transform,
            &Pet,
            Option<&ActiveAuras>,
            Option<&PetCommand>,
        ),
        (Without<CastingState>, Without<ChannelingState>),
    >,
    casting_targets: Query<(Entity, &Combatant, &CastingState), Without<Pet>>,
    channeling_targets: Query<
        (Entity, &Combatant, &ChannelingState),
        (Without<CastingState>, Without<Pet>),
    >,
    all_combatants: Query<(Entity, &Combatant, &Transform, Option<&ActiveAuras>), Without<Pet>>,
    dr_tracker_query: Query<(Entity, &DRTracker)>,
    celebration: Option<Res<VictoryCelebration>>,
    map_geometry: Res<crate::states::play_match::map_config::ActiveMapGeometry>,
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
            (
                entity,
                super::CombatantInfo {
                    entity,
                    team: combatant.team,
                    slot: combatant.slot,
                    class: combatant.class,
                    current_health: combatant.current_health,
                    max_health: combatant.max_health,
                    current_mana: combatant.current_mana,
                    max_mana: combatant.max_mana,
                    position: transform.translation,
                    velocity: Vec3::ZERO,
                    is_alive: combatant.is_alive(),
                    stealthed: combatant.stealthed,
                    target: combatant.target,
                    is_pet: false,
                    // Pet AI doesn't read casts; this coarse snapshot omits CastingState.
                    casting_ability: None,
                    pet_type: None,
                    pet: owner_to_pet.get(&entity).copied(),
                },
            )
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
    let ability_cooldowns: std::collections::BTreeMap<
        Entity,
        std::collections::BTreeMap<crate::states::play_match::abilities::AbilityType, f32>,
    > = all_combatants
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
            combatant.target = combatant_info
                .get(&pet.owner)
                .and_then(|owner_info| owner_info.target);
        }

        let my_pos = transform.translation;
        let ctx = CombatContext::new(
            entity,
            combatant.team,
            &combatant_info,
            &active_auras_map,
            &dr_trackers,
            &ability_cooldowns,
            &map_geometry.volumes,
            map_geometry.bounds,
            // Pets have no team-level behaviours; Legacy is always correct here.
            Default::default(),
        );

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

        // U4: Hunter-dispatched PetCommand execution. Runs before the
        // autonomous decide path; on completion the autonomous path is skipped
        // (continue) since the pet's GCD/ability slot for this tick is owned
        // by the dispatched ability.
        //
        // Authoritative checks at execution time (the "optimistic dispatch"
        // contract per the plan's Key Technical Decisions): Hunter uses
        // snapshot heuristics to spawn the PetCommand; pet AI re-validates
        // here with live `&Combatant` state. If conditions changed since
        // dispatch (cooldown rolled, target died, friendly CC landed), the
        // command is rejected and despawned without firing.
        if let Some(command) = pet_command.copied() {
            let dispatch_target_view = ctx
                .combatants
                .get(&command.target)
                .map(|info| TargetView::from_info(info, my_pos));
            let mut builder = decision_trace.start_pet_dispatch_decision(
                actor_view.clone(),
                dispatch_target_view,
                pet.owner,
                pet_type_str(pet.pet_type),
                command.dispatched_by,
            );

            let ability = command.ability;
            if let Some(def) = abilities.get(&ability) {
                let rejection =
                    pet_command_rejection(ability, def, &combatant, my_pos, command.target, &ctx);
                if let Some(reason) = rejection {
                    builder.reject(ability, reason);
                } else {
                    builder.choose(ability, Some(command.target), true);
                    match ability {
                        AbilityType::SpiderWeb => execute_spider_web(
                            &mut commands,
                            &mut combat_log,
                            def,
                            &abilities,
                            entity,
                            &mut combatant,
                            my_pos,
                            command.target,
                        ),
                        AbilityType::BoarCharge => execute_boar_charge(
                            &mut commands,
                            &mut combat_log,
                            def,
                            &abilities,
                            entity,
                            &mut combatant,
                            command.target,
                        ),
                        AbilityType::MastersCall => execute_masters_call(
                            &mut commands,
                            &mut combat_log,
                            def,
                            &abilities,
                            entity,
                            &mut combatant,
                            command.target,
                        ),
                        _ => {
                            // Unsupported ability via PetCommand. Drop with no
                            // execution; the builder's `choose` is already set
                            // so the trace will record an unintended cast —
                            // this path should be unreachable in normal flow.
                        }
                    }
                }
            } else {
                builder.reject(
                    ability,
                    RejectionReason::PreconditionUnmet {
                        note: "missing ability def".to_string(),
                    },
                );
            }

            builder.finish();
            commands.entity(entity).remove::<PetCommand>();
            continue;
        }

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

        // Per-pet autonomous decide. Headline pet abilities (Spider Web, Boar
        // Charge, Master's Call) are Hunter-dispatched via PetCommand when
        // Hunter AI is eligible to run. These autonomous fallbacks fire when
        // no PetCommand was queued this tick — covers the case where Hunter
        // is mid-cast (CastingState filters Hunter out of `decide_abilities`),
        // ensuring the pet's headline ability isn't starved of opportunities
        // during Hunter's Aimed Shot windows. Both paths share the same
        // execute_* helpers, so behavior matches except for the dispatched_by
        // trace attribution. Heel + CD + AlreadyApplied + friendly-CC checks
        // mirror the Hunter-dispatch predicates.
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
                    &mut builder,
                );
            }
            PetType::Spider => {
                spider_autonomous_dispatch(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    entity,
                    &mut combatant,
                    my_pos,
                    pet,
                    &ctx,
                    &mut builder,
                );
            }
            PetType::Boar => {
                boar_autonomous_dispatch(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    entity,
                    &mut combatant,
                    my_pos,
                    pet,
                    &ctx,
                    &mut builder,
                );
            }
            PetType::Bird => {
                bird_autonomous_dispatch(
                    &mut commands,
                    &mut combat_log,
                    &abilities,
                    entity,
                    &mut combatant,
                    my_pos,
                    pet,
                    &ctx,
                    &mut builder,
                );
            }
        }

        builder.finish();
    }
}

/// Authoritative pre-execution checks for a Hunter-dispatched PetCommand.
/// Returns the rejection reason if any check fails, or `None` if the command
/// is OK to execute. Mirrors what `pre_cast_ok` does for class-AI casts but
/// scoped to the predicates that matter for pet headline abilities.
fn pet_command_rejection(
    ability: AbilityType,
    def: &crate::states::play_match::ability_config::AbilityConfig,
    combatant: &Combatant,
    my_pos: Vec3,
    target: Entity,
    ctx: &CombatContext,
) -> Option<RejectionReason> {
    if combatant.global_cooldown > 0.0 {
        return Some(RejectionReason::OnCooldown {
            remaining: combatant.global_cooldown,
        });
    }
    if let Some(remaining) = combatant.ability_cooldowns.get(&ability) {
        return Some(RejectionReason::OnCooldown {
            remaining: *remaining,
        });
    }

    let Some(target_info) = ctx.combatants.get(&target) else {
        return Some(RejectionReason::NoValidTarget);
    };
    if !target_info.is_alive {
        return Some(RejectionReason::NoValidTarget);
    }

    if matches!(ability, AbilityType::SpiderWeb | AbilityType::BoarCharge) {
        let dist = my_pos.distance(target_info.position);
        if dist > def.range {
            return Some(RejectionReason::OutOfRange {
                distance: dist,
                max: def.range,
            });
        }
        if ability == AbilityType::BoarCharge && dist < super::super::constants::CHARGE_MIN_RANGE {
            return Some(RejectionReason::WithinDeadZone {
                distance: dist,
                min: super::super::constants::CHARGE_MIN_RANGE,
            });
        }
        // Friendly-CC guard only applies to abilities that deal damage on
        // landing — Spider Web is a 0-damage Root and can't break a friendly
        // CC. Boar Charge's impact damage would break threshold-0 auras
        // (Polymorph, Freezing Trap incap).
        if ability == AbilityType::BoarCharge && ctx.has_friendly_breakable_cc(target) {
            return Some(RejectionReason::FriendlyBreakableCC);
        }
    }

    // Master's Call frees a teammate, and no ally-freeing cast goes through a
    // pillar: re-check the bird's sight of the recipient, which may have been
    // lost since the Hunter dispatched it. (Range was the dispatch's check and
    // is not re-asked here.)
    if ability == AbilityType::MastersCall
        && !has_line_of_sight(ctx.obstacles, my_pos, target_info.position)
    {
        return Some(RejectionReason::LosBlocked);
    }

    None
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
    channeling_targets: &Query<
        (Entity, &Combatant, &ChannelingState),
        (Without<CastingState>, Without<Pet>),
    >,
    builder: &mut DecisionEventBuilder<'_>,
) {
    if combatant.global_cooldown > 0.0 {
        return;
    }

    if try_spell_lock(
        commands,
        combat_log,
        abilities,
        entity,
        combatant,
        my_pos,
        ctx,
        casting_targets,
        channeling_targets,
        builder,
    ) {
        return;
    }

    try_devour_magic(
        commands, combat_log, abilities, entity, combatant, my_pos, ctx, builder,
    );
}

/// The cast Spell Lock interrupts, from every enemy cast it reaches (each with
/// whether it is a heal), in query order: an enemy Druid's Cyclone first
/// ([`super::priority_interrupt_target`], every interrupter's first pick), then
/// the first heal, then the first cast of any kind.
pub fn spell_lock_target(casts: &[(super::InterruptCandidate, bool)]) -> Option<Entity> {
    super::priority_interrupt_target(casts.iter().map(|(cast, _)| *cast))
        .or_else(|| casts.iter().find(|(_, heal)| *heal).map(|(c, _)| c.entity))
        .or_else(|| casts.first().map(|(c, _)| c.entity))
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
    channeling_targets: &Query<
        (Entity, &Combatant, &ChannelingState),
        (Without<CastingState>, Without<Pet>),
    >,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::SpellLock;
    let def = abilities.get_unchecked(&ability);

    if let Some(remaining) = combatant.ability_cooldowns.get(&ability) {
        builder.reject(
            ability,
            RejectionReason::OnCooldown {
                remaining: *remaining,
            },
        );
        return false;
    }

    let my_team = combatant.team;

    // Collect every interruptible enemy cast in range, then pick the highest-value
    // one. Spell Lock applies a *school-specific* lockout, so interrupting a heal
    // locks the healer out of healing — far more valuable than eating a DPS nuke.
    // Priority: an enemy Druid's Cyclone (`super::priority_interrupt_target`,
    // every interrupter's first pick) > heal cast > any other cast (first-seen).
    // This turns the Felhunter's one interrupt per 24s into a heal-denial tool
    // instead of a random interrupt.
    let mut reachable: Vec<(super::InterruptCandidate, bool)> = Vec::new();
    for (target_entity, target_combatant, cast_state) in casting_targets.iter() {
        if target_combatant.team == my_team || !target_combatant.is_alive() {
            continue;
        }
        if !super::cast_is_interruptible(cast_state, abilities) {
            continue;
        }
        if ctx.entity_is_immune(target_entity) {
            continue;
        }
        let distance = my_pos.distance(
            ctx.combatants
                .get(&target_entity)
                .map(|i| i.position)
                .unwrap_or(Vec3::ZERO),
        );
        if distance > def.range {
            continue;
        }
        reachable.push((
            super::InterruptCandidate {
                entity: target_entity,
                ability: cast_state.ability,
                distance,
            },
            abilities.get_unchecked(&cast_state.ability).is_heal(),
        ));
    }
    if let Some(target_entity) = spell_lock_target(&reachable) {
        builder.choose(ability, Some(target_entity), true);
        execute_spell_lock(
            commands,
            combat_log,
            abilities,
            entity,
            combatant,
            target_entity,
            &def.name,
        );
        return true;
    }

    for (target_entity, target_combatant, _) in channeling_targets.iter() {
        if target_combatant.team == my_team || !target_combatant.is_alive() {
            continue;
        }
        if ctx.entity_is_immune(target_entity) {
            continue;
        }
        let distance = my_pos.distance(
            ctx.combatants
                .get(&target_entity)
                .map(|i| i.position)
                .unwrap_or(Vec3::ZERO),
        );
        if distance > def.range {
            continue;
        }
        builder.choose(ability, Some(target_entity), true);
        execute_spell_lock(
            commands,
            combat_log,
            abilities,
            entity,
            combatant,
            target_entity,
            &def.name,
        );
        return true;
    }

    builder.reject(ability, RejectionReason::NoValidTarget);
    false
}

pub(crate) fn execute_spell_lock(
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

    combatant.start_cooldown(ability, abilities);

    let caster_id = pet_combatant_id(
        combatant.team,
        combatant.owner_relative_slot(),
        PetType::Felhunter,
    );
    combat_log.log_ability_cast(
        caster_id.clone(),
        ability_name.to_string(),
        None,
        format!("{} uses {}", caster_id, ability_name),
    );

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
        builder.reject(
            ability,
            RejectionReason::OnCooldown {
                remaining: *remaining,
            },
        );
        return false;
    }

    // What Devour takes, and whom: the one classification the Hunter's trap AI
    // also asks (`ally_removal`), so the cast and that question cannot drift.
    let removal = super::ally_removal(ability).expect("Devour Magic frees allies");

    let my_team = combatant.team;
    let mut best_target: Option<(Entity, Vec3)> = None;
    // A teammate with something to devour that stands in range but out of
    // sight — reported as `LosBlocked` when no reachable teammate qualifies.
    let mut los_blocked = false;

    for (ally_entity, info) in ctx.combatants.iter() {
        if info.team != my_team || !info.is_alive || (info.is_pet && !removal.reaches_pets) {
            continue;
        }
        // No dispel reaches a cycloned teammate (`scan_ally_dispel`'s rule).
        if ctx.is_cycloned(*ally_entity) {
            continue;
        }
        let has_dispellable = ctx
            .active_auras
            .get(ally_entity)
            .map(|auras| auras.iter().any(|a| removal.scope.takes(a)))
            .unwrap_or(false);
        if !has_dispellable {
            continue;
        }
        // Range, then line of sight (`ally_reach`, the gates every ally dispel
        // passes).
        match super::ally_reach(ctx, def.range, my_pos, info.position) {
            super::AllyReach::Reaches => {}
            super::AllyReach::OutOfRange { .. } => continue,
            super::AllyReach::LosBlocked => {
                los_blocked = true;
                continue;
            }
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
        let reason = if los_blocked {
            RejectionReason::LosBlocked
        } else {
            RejectionReason::NoValidTarget
        };
        builder.reject(ability, reason);
        return false;
    };

    builder.choose(ability, Some(target_entity), true);

    combatant.start_cooldown(ability, abilities);
    combatant.global_cooldown = super::super::constants::GCD;

    let caster_id = pet_combatant_id(
        combatant.team,
        combatant.owner_relative_slot(),
        PetType::Felhunter,
    );
    combat_log.log_ability_cast(
        caster_id.clone(),
        def.name.to_string(),
        None,
        format!("{} uses {}", caster_id, def.name),
    );

    let heal_amount = combatant.max_health * 0.10;

    commands.spawn(DispelPending {
        target: target_entity,
        dispeller: entity,
        log_prefix: "[DEVOUR]",
        caster_class: CharacterClass::Warlock,
        heal_on_success: Some((entity, heal_amount)),
        scope: removal.scope,
    });

    true
}

// ==============================================================================
// Pet ability execution helpers (Hunter-dispatched via PetCommand)
// ==============================================================================

/// Spawn the Spider Web projectile at the spider, set CD/GCD, log the cast.
/// Called from `pet_ai_system` after authoritative pre-execution checks pass.
pub(crate) fn execute_spider_web(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    def: &crate::states::play_match::ability_config::AbilityConfig,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    target: Entity,
) {
    let ability = AbilityType::SpiderWeb;
    let projectile_speed = def.projectile_speed.unwrap_or(50.0);
    commands.spawn((
        Projectile {
            caster: entity,
            target,
            ability,
            speed: projectile_speed,
            caster_team: combatant.team,
            caster_slot: combatant.owner_relative_slot(),
            caster_class: combatant.class,
            caster_pet_type: Some(PetType::Spider),
        },
        Transform::from_translation(my_pos + Vec3::new(0.0, 0.5, 0.0)),
        PlayMatchEntity,
    ));

    combatant.start_cooldown(ability, abilities);
    combatant.global_cooldown = super::super::constants::GCD;

    let caster_id = pet_combatant_id(
        combatant.team,
        combatant.owner_relative_slot(),
        PetType::Spider,
    );
    combat_log.log_ability_cast(
        caster_id.clone(),
        def.name.to_string(),
        None,
        format!("{} uses {}", caster_id, def.name),
    );
}

/// Apply Boar Charge to a target: ChargingState marker + delayed Stun aura,
/// set CD/GCD, log the cast.
pub(crate) fn execute_boar_charge(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    def: &crate::states::play_match::ability_config::AbilityConfig,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    target: Entity,
) {
    let ability = AbilityType::BoarCharge;
    commands.entity(entity).try_insert(ChargingState { target });

    if let Some(aura_pending) = AuraPending::from_ability(target, entity, def) {
        commands.spawn((aura_pending, PlayMatchEntity));
    }

    combatant.start_cooldown(ability, abilities);
    combatant.global_cooldown = super::super::constants::GCD;

    let caster_id = pet_combatant_id(
        combatant.team,
        combatant.owner_relative_slot(),
        PetType::Boar,
    );
    combat_log.log_ability_cast(
        caster_id.clone(),
        def.name.to_string(),
        None,
        format!("{} uses {}", caster_id, def.name),
    );
}

/// Apply Master's Call to a target: spawn DispelPending + DispelBurst, set
/// CD/GCD, log the cast. Caller is responsible for verifying the target has
/// at least one dispellable Root/MovementSpeedSlow aura.
pub(crate) fn execute_masters_call(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    def: &crate::states::play_match::ability_config::AbilityConfig,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    target: Entity,
) {
    let ability = AbilityType::MastersCall;
    commands.spawn(DispelPending {
        target,
        dispeller: entity,
        log_prefix: "[MASTERS_CALL]",
        caster_class: CharacterClass::Hunter,
        heal_on_success: None,
        scope: DispelScope::Impairments(super::MASTERS_CALL_IMPAIRMENTS.to_vec()),
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

    combatant.start_cooldown(ability, abilities);
    combatant.global_cooldown = super::super::constants::GCD;

    let caster_id = pet_combatant_id(
        combatant.team,
        combatant.owner_relative_slot(),
        PetType::Bird,
    );
    combat_log.log_ability_cast(
        caster_id.clone(),
        def.name.to_string(),
        None,
        format!("{} uses {}", caster_id, def.name),
    );
}

// ==============================================================================
// Autonomous pet dispatch fallbacks (fire when no PetCommand queued)
// ==============================================================================
//
// These mirror the Hunter-side `try_dispatch_*` helpers' predicate logic but
// run inside `pet_ai_system` so they fire even when Hunter is mid-cast (the
// `Without<CastingState>` filter on `decide_abilities` would otherwise gate
// dispatch). Both paths share the same execute_* helpers; the only difference
// in the resulting trace is `dispatched_by` (set by Hunter, omitted by these
// autonomous paths).

/// Autonomous Spider Web dispatch — fires on the owner's target if conditions
/// hold. Skips silently if the pet has no eligible target (e.g., owner has no
/// target or target is out of range). Cooldown/heel/already-rooted are
/// emitted as candidate rejections so the trace remains attributable.
fn spider_autonomous_dispatch(
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
    let Some(def) = abilities.get(&ability) else {
        return;
    };

    // Heel suppression — pet AI already handled HP<25% via continue above,
    // but defensively skip dispatch if the pet is heeling.
    let hp_ratio = if combatant.max_health > 0.0 {
        combatant.current_health / combatant.max_health
    } else {
        0.0
    };
    if hp_ratio < 0.25 {
        return;
    }

    if let Some(remaining) = combatant.ability_cooldowns.get(&ability) {
        builder.reject(
            ability,
            RejectionReason::OnCooldown {
                remaining: *remaining,
            },
        );
        return;
    }

    let Some(owner_info) = ctx.combatants.get(&pet.owner) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    };
    let Some(target) = owner_info.target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    };
    let Some(target_info) = ctx.combatants.get(&target) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    };
    if !target_info.is_alive || target_info.is_pet || target_info.team == combatant.team {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    }

    let dist = my_pos.distance(target_info.position);
    if dist > def.range {
        builder.reject(
            ability,
            RejectionReason::OutOfRange {
                distance: dist,
                max: def.range,
            },
        );
        return;
    }

    if let Some(auras) = ctx.active_auras.get(&target) {
        if auras.iter().any(|a| a.effect_type == AuraType::Root) {
            builder.reject(ability, RejectionReason::AlreadyApplied);
            return;
        }
    }

    builder.choose(ability, Some(target), true);
    execute_spider_web(
        commands, combat_log, def, abilities, entity, combatant, my_pos, target,
    );
}

/// Autonomous Boar Charge dispatch. Friendly-CC guard applies here because
/// charge deals impact damage (would break threshold-0 friendly CC).
fn boar_autonomous_dispatch(
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
    let ability = AbilityType::BoarCharge;
    let Some(def) = abilities.get(&ability) else {
        return;
    };

    let hp_ratio = if combatant.max_health > 0.0 {
        combatant.current_health / combatant.max_health
    } else {
        0.0
    };
    if hp_ratio < 0.25 {
        return;
    }

    if let Some(remaining) = combatant.ability_cooldowns.get(&ability) {
        builder.reject(
            ability,
            RejectionReason::OnCooldown {
                remaining: *remaining,
            },
        );
        return;
    }

    let Some(owner_info) = ctx.combatants.get(&pet.owner) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    };
    let Some(target) = owner_info.target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    };
    let Some(target_info) = ctx.combatants.get(&target) else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    };
    if !target_info.is_alive || target_info.is_pet || target_info.team == combatant.team {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    }

    let dist = my_pos.distance(target_info.position);
    if dist > def.range {
        builder.reject(
            ability,
            RejectionReason::OutOfRange {
                distance: dist,
                max: def.range,
            },
        );
        return;
    }
    if dist < super::super::constants::CHARGE_MIN_RANGE {
        builder.reject(
            ability,
            RejectionReason::WithinDeadZone {
                distance: dist,
                min: super::super::constants::CHARGE_MIN_RANGE,
            },
        );
        return;
    }
    if ctx.has_friendly_breakable_cc(target) {
        builder.reject(ability, RejectionReason::FriendlyBreakableCC);
        return;
    }

    builder.choose(ability, Some(target), true);
    execute_boar_charge(
        commands, combat_log, def, abilities, entity, combatant, target,
    );
}

/// Autonomous Master's Call dispatch. Cleanses Root/MovementSpeedSlow from
/// the owner first, then scans allies. Mirrors `try_dispatch_masters_call`.
fn bird_autonomous_dispatch(
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
    let ability = AbilityType::MastersCall;
    let Some(def) = abilities.get(&ability) else {
        return;
    };

    let hp_ratio = if combatant.max_health > 0.0 {
        combatant.current_health / combatant.max_health
    } else {
        0.0
    };
    if hp_ratio < 0.25 {
        return;
    }

    if let Some(remaining) = combatant.ability_cooldowns.get(&ability) {
        builder.reject(
            ability,
            RejectionReason::OnCooldown {
                remaining: *remaining,
            },
        );
        return;
    }

    let owner_needs_cleanse = ctx.active_auras.get(&pet.owner).is_some_and(|auras| {
        auras
            .iter()
            .any(|a| matches!(a.effect_type, AuraType::Root | AuraType::MovementSpeedSlow,))
    });
    let target = if owner_needs_cleanse {
        Some(pet.owner)
    } else {
        let mut fallback: Option<Entity> = None;
        for (ally_entity, info) in ctx.combatants.iter() {
            if info.team != combatant.team || !info.is_alive || info.is_pet {
                continue;
            }
            if let Some(auras) = ctx.active_auras.get(ally_entity) {
                if auras
                    .iter()
                    .any(|a| matches!(a.effect_type, AuraType::Root | AuraType::MovementSpeedSlow,))
                {
                    fallback = Some(*ally_entity);
                    break;
                }
            }
        }
        fallback
    };

    let Some(target) = target else {
        builder.reject(ability, RejectionReason::NoValidTarget);
        return;
    };

    // Range, then line of sight from the bird to the cleanse recipient
    // (`ally_reach`, the gates every ally-freeing cast passes).
    if let Some(target_info) = ctx.combatants.get(&target) {
        let reason = match super::ally_reach(ctx, def.range, my_pos, target_info.position) {
            super::AllyReach::Reaches => None,
            super::AllyReach::OutOfRange { distance } => Some(RejectionReason::OutOfRange {
                distance,
                max: def.range,
            }),
            super::AllyReach::LosBlocked => Some(RejectionReason::LosBlocked),
        };
        if let Some(reason) = reason {
            builder.reject(ability, reason);
            return;
        }
    }

    builder.choose(ability, Some(target), true);
    execute_masters_call(
        commands, combat_log, def, abilities, entity, combatant, target,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::class_ai::CombatantInfo;
    use crate::states::play_match::map_geometry::ObstacleVolume;
    use crate::states::play_match::traps::freezing_trap_aura;
    use bevy::ecs::world::CommandQueue;
    use std::collections::BTreeMap;

    /// Devour Magic frees a trapped teammate it can see, and refuses — traced
    /// `LosBlocked` — the same teammate at the same distance behind a pillar
    /// (AS-186: no ally dispel goes through a pillar).
    #[test]
    fn devour_magic_needs_sight_of_the_teammate() {
        let unit = |entity, class, position, is_pet| CombatantInfo {
            entity,
            team: 1,
            slot: 0,
            class,
            current_health: 100.0,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position,
            velocity: Vec3::ZERO,
            is_alive: true,
            stealthed: false,
            target: None,
            is_pet,
            casting_ability: None,
            pet_type: is_pet.then_some(PetType::Felhunter),
            pet: None,
        };
        let (felhunter, warrior) = (Entity::from_raw(1), Entity::from_raw(2));
        let (pet_pos, warrior_pos) = (Vec3::new(0.0, 1.0, 0.0), Vec3::new(16.0, 1.0, 0.0));
        let roster: BTreeMap<_, _> = [
            (
                felhunter,
                unit(felhunter, CharacterClass::Warlock, pet_pos, true),
            ),
            (
                warrior,
                unit(warrior, CharacterClass::Warrior, warrior_pos, false),
            ),
        ]
        .into_iter()
        .collect();
        let mut auras = BTreeMap::new();
        auras.insert(warrior, vec![freezing_trap_aura(Entity::from_raw(9))]);
        let (dr, cds) = (BTreeMap::new(), BTreeMap::new());
        let pillar = [ObstacleVolume::Cylinder {
            center_xz: Vec2::new(8.0, 0.0),
            radius: 2.0,
            base_y: 0.0,
            height: 10.0,
        }];

        let devour = |obstacles: &[ObstacleVolume]| {
            let ctx = CombatContext::new(
                felhunter,
                1,
                &roster,
                &auras,
                &dr,
                &cds,
                obstacles,
                Default::default(),
                Default::default(),
            );
            let world = World::new();
            let mut queue = CommandQueue::default();
            let mut commands = Commands::new(&mut queue, &world);
            let mut combat_log = CombatLog::default();
            let mut combatant = Combatant::new(1, 0, CharacterClass::Warlock);
            let mut trace = DecisionTrace::default();
            let mut builder = trace.start_pet_decision(
                ActorView::from_info(&roster[&felhunter]),
                None,
                Entity::from_raw(3),
                "Felhunter",
            );
            let cast = try_devour_magic(
                &mut commands,
                &mut combat_log,
                &AbilityDefinitions::default(),
                felhunter,
                &mut combatant,
                pet_pos,
                &ctx,
                &mut builder,
            );
            builder.finish();
            let event = serde_json::to_value(&trace.pending_events[0]).unwrap();
            let candidate = event["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["ability"] == "DevourMagic")
                .cloned()
                .unwrap();
            (cast, candidate)
        };

        let (cast, candidate) = devour(&[]);
        assert!(cast, "in sight: Devour Magic frees the Warrior");
        assert_eq!(candidate["status"], "chosen");

        let (cast, candidate) = devour(&pillar);
        assert!(!cast, "behind the pillar: no Devour Magic through it");
        assert_eq!(candidate["status"], "rejected");
        assert_eq!(candidate["reason"], "LosBlocked");
    }

    /// Master's Call frees a rooted Hunter its Bird can see, and refuses — traced
    /// `LosBlocked` — the same Hunter at the same distance behind a pillar, on
    /// every path that casts it: the Bird's own dispatch
    /// (`bird_autonomous_dispatch`), the Hunter's dispatch
    /// (`try_dispatch_masters_call`), and the Bird's authoritative re-check of a
    /// dispatched command whose sightline closed after it was issued
    /// (`pet_command_rejection`).
    #[test]
    fn masters_call_needs_sight_of_the_teammate() {
        let unit = |entity, class, position, pet_type: Option<PetType>| CombatantInfo {
            entity,
            team: 1,
            slot: 0,
            class,
            current_health: 100.0,
            max_health: 100.0,
            current_mana: 100.0,
            max_mana: 100.0,
            position,
            velocity: Vec3::ZERO,
            is_alive: true,
            stealthed: false,
            target: None,
            is_pet: pet_type.is_some(),
            casting_ability: None,
            pet_type,
            pet: None,
        };
        let (bird, hunter) = (Entity::from_raw(1), Entity::from_raw(2));
        let (bird_pos, hunter_pos) = (Vec3::new(0.0, 1.0, 0.0), Vec3::new(16.0, 1.0, 0.0));
        let roster: BTreeMap<_, _> = [
            (
                bird,
                unit(bird, CharacterClass::Hunter, bird_pos, Some(PetType::Bird)),
            ),
            (
                hunter,
                unit(hunter, CharacterClass::Hunter, hunter_pos, None),
            ),
        ]
        .into_iter()
        .collect();
        let mut root = freezing_trap_aura(Entity::from_raw(9));
        root.effect_type = AuraType::Root;
        let mut auras = BTreeMap::new();
        auras.insert(hunter, vec![root.clone()]);
        let hunter_auras = ActiveAuras { auras: vec![root] };
        let (dr, cds) = (BTreeMap::new(), BTreeMap::new());
        let pillar = [ObstacleVolume::Cylinder {
            center_xz: Vec2::new(8.0, 0.0),
            radius: 2.0,
            base_y: 0.0,
            height: 10.0,
        }];
        let abilities = AbilityDefinitions::default();
        let def = abilities.get(&AbilityType::MastersCall).unwrap();
        assert!(
            hunter_pos.distance(bird_pos) < def.range,
            "range never decides it"
        );

        let candidate = |trace: &DecisionTrace| {
            let event = serde_json::to_value(trace.pending_events.last().unwrap()).unwrap();
            event["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["ability"] == "MastersCall")
                .cloned()
                .unwrap()
        };

        // The Bird's own dispatch.
        let autonomous = |obstacles: &[ObstacleVolume]| {
            let ctx = CombatContext::new(
                bird,
                1,
                &roster,
                &auras,
                &dr,
                &cds,
                obstacles,
                Default::default(),
                Default::default(),
            );
            let world = World::new();
            let mut queue = CommandQueue::default();
            let mut commands = Commands::new(&mut queue, &world);
            let mut combat_log = CombatLog::default();
            let mut combatant = Combatant::new(1, 0, CharacterClass::Hunter);
            let mut trace = DecisionTrace::default();
            let mut builder = trace.start_pet_decision(
                ActorView::from_info(&roster[&bird]),
                None,
                hunter,
                "Bird",
            );
            bird_autonomous_dispatch(
                &mut commands,
                &mut combat_log,
                &abilities,
                bird,
                &mut combatant,
                bird_pos,
                &Pet {
                    owner: hunter,
                    pet_type: PetType::Bird,
                },
                &ctx,
                &mut builder,
            );
            builder.finish();
            candidate(&trace)
        };
        // The Hunter's dispatch.
        let dispatched = |obstacles: &[ObstacleVolume]| {
            let ctx = CombatContext::new(
                hunter,
                1,
                &roster,
                &auras,
                &dr,
                &cds,
                obstacles,
                Default::default(),
                Default::default(),
            );
            let world = World::new();
            let mut queue = CommandQueue::default();
            let mut commands = Commands::new(&mut queue, &world);
            let mut trace = DecisionTrace::default();
            let sent = super::super::hunter::try_dispatch_masters_call(
                &mut commands,
                &abilities,
                &mut trace,
                hunter,
                bird,
                &roster[&bird],
                &roster[&hunter],
                Some(&hunter_auras),
                &ctx,
            );
            (sent, candidate(&trace))
        };
        // A command dispatched in sight, re-checked once the pillar is between.
        let recheck = |obstacles: &[ObstacleVolume]| {
            let ctx = CombatContext::new(
                bird,
                1,
                &roster,
                &auras,
                &dr,
                &cds,
                obstacles,
                Default::default(),
                Default::default(),
            );
            let combatant = Combatant::new(1, 0, CharacterClass::Hunter);
            pet_command_rejection(
                AbilityType::MastersCall,
                def,
                &combatant,
                bird_pos,
                hunter,
                &ctx,
            )
        };

        assert_eq!(autonomous(&[])["status"], "chosen", "Bird, in sight");
        let behind = autonomous(&pillar);
        assert_eq!(behind["status"], "rejected");
        assert_eq!(behind["reason"], "LosBlocked", "Bird, behind the pillar");

        let (sent, open) = dispatched(&[]);
        assert!(sent, "Hunter dispatches it in sight");
        assert_eq!(open["status"], "chosen");
        let (sent, behind) = dispatched(&pillar);
        assert!(!sent, "Hunter holds it behind the pillar");
        assert_eq!(behind["reason"], "LosBlocked");

        assert!(recheck(&[]).is_none(), "a command in sight executes");
        assert!(
            matches!(recheck(&pillar), Some(RejectionReason::LosBlocked)),
            "a command whose sightline closed is refused"
        );
    }
}
