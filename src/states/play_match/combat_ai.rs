//! Combat AI Systems
//!
//! Handles all AI decision-making for combatants:
//! - Target acquisition (choosing which enemy to fight)
//! - Ability decisions (class-specific AI for using abilities)
//! - Interrupt decisions (when to interrupt enemy casts)

use super::abilities::AbilityType;
use super::class_ai;
use super::components::*;
use super::constants::{TRAP_ARM_DELAY, TRAP_LAUNCH_SPEED};
use super::match_config;
use super::situation::{update_situations, MatchClock, SituationConfig, Situations};
use super::utils::{combat_log_id_for, combatant_id, get_next_fct_offset};
use crate::combat::log::CombatLog;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// Bundled extra params for `decide_abilities`, keeping it within Bevy's
/// 16-argument system-function limit. Holds the victory-celebration guard and
/// the live `Totem` query (so the Shaman AI knows which of its element totems
/// are already out / about to expire).
#[derive(SystemParam)]
pub struct AbilityDispatchExtras<'w, 's> {
    celebration: Option<Res<'w, VictoryCelebration>>,
    totems: Query<'w, 's, &'static Totem>,
    /// Active map obstacle geometry — bundled here (rather than a bare
    /// `Res` argument) so `decide_abilities` stays within Bevy's 16
    /// system-param limit. Fed to `CombatSnapshot::build` for the cast-start
    /// LoS guard.
    map_geometry: Res<'w, super::map_config::ActiveMapGeometry>,
    /// Which AI implementation this match runs under. `Option` so a scene that
    /// never inserted it falls back to `Legacy` rather than panicking.
    ai_profile: Option<Res<'w, super::ai_profile::AiProfiles>>,
    /// Freezing Traps on the ground and in flight, so a Hunter's damage AI can
    /// see its own traps that have not sprung yet.
    traps: Query<'w, 's, (&'static Trap, &'static Transform)>,
    trap_launches: Query<'w, 's, &'static TrapLaunchProjectile>,
    /// Arena dampening — read for its `time_since_gates`, the clock under the
    /// situations' match phase. `Option` for the same reason as `ai_profile`.
    dampening: Option<Res<'w, ArenaDampening>>,
    /// The match's configured kill targets, so a Hunter knows whether the
    /// enemy it cannot see is the one its team will converge on. `Option` so
    /// a scene that never inserted it plays as if none was set.
    config: Option<Res<'w, match_config::MatchConfig>>,
    /// Lit Flares, so a stealthed Rogue can play around the light it sees.
    flares: Query<'w, 's, (Entity, &'static FlareZone, &'static Transform)>,
    /// Each team's situation, kept current here (`situation.rs`). `Option` so a
    /// scene that never inserted it still classifies, with no history.
    situations: Option<ResMut<'w, Situations>>,
    /// The gated plays each team is denied, for measurement. `Option`: absent
    /// (the graphical client, a scene) withholds nothing.
    withheld_plays: Option<Res<'w, super::situation::WithheldPlays>>,
}

pub fn acquire_targets(
    countdown: Res<MatchCountdown>,
    config: Res<match_config::MatchConfig>,
    movement_config: Res<crate::states::play_match::movement_config::MovementConfig>,
    mut combatants: Query<(Entity, &mut Combatant, &Transform, Option<&ActiveAuras>)>,
    pet_query: Query<&Pet>,
    mut decision_trace: ResMut<crate::states::play_match::decision_trace::DecisionTrace>,
    // The team's target focus (AS-389), as `decide_abilities` classified it
    // last frame, and the map it is judged on. `Option`: absent (a test app)
    // follows no focus.
    situations: Option<Res<Situations>>,
    map_geometry: Option<Res<super::map_config::ActiveMapGeometry>>,
) {
    // Don't acquire targets until gates open
    if !countdown.gates_opened {
        return;
    }

    // First pass: identify which entities have Shadow Sight
    let shadow_sight_holders: std::collections::HashSet<Entity> = combatants
        .iter()
        .filter_map(|(entity, _, _, auras)| {
            if let Some(active) = auras {
                if active
                    .auras
                    .iter()
                    .any(|a| a.effect_type == AuraType::ShadowSight)
                {
                    return Some(entity);
                }
            }
            None
        })
        .collect();

    // Build list of all alive combatants with their info
    // Tuple: (entity, position, stealthed, has_shadow_sight, class, current_health, is_immune, is_pet)
    let mut team1_combatants: Vec<(
        Entity,
        Vec3,
        bool,
        bool,
        match_config::CharacterClass,
        f32,
        bool,
        bool,
    )> = Vec::new();
    let mut team2_combatants: Vec<(
        Entity,
        Vec3,
        bool,
        bool,
        match_config::CharacterClass,
        f32,
        bool,
        bool,
    )> = Vec::new();

    // Collect active auras for CC checking
    let active_auras_map: std::collections::HashMap<Entity, Vec<Aura>> = combatants
        .iter()
        .filter_map(|(entity, _, _, auras_opt)| {
            auras_opt.map(|auras| (entity, auras.auras.clone()))
        })
        .collect();

    for (entity, c, transform, _) in combatants.iter() {
        if !c.is_alive() {
            continue;
        }

        let has_shadow_sight = shadow_sight_holders.contains(&entity);
        // Divine Shield or a Cyclone: the one immunity predicate the class AIs
        // ask too, so an attacker switches off a cycloned target.
        let is_immune = active_auras_map.get(&entity).is_some_and(|auras| {
            auras
                .iter()
                .any(|a| class_ai::grants_damage_immunity(a.effect_type))
        });
        let is_pet = pet_query.get(entity).is_ok();

        if c.team == 1 {
            team1_combatants.push((
                entity,
                transform.translation,
                c.stealthed,
                has_shadow_sight,
                c.class,
                c.current_health,
                is_immune,
                is_pet,
            ));
        } else {
            team2_combatants.push((
                entity,
                transform.translation,
                c.stealthed,
                has_shadow_sight,
                c.class,
                c.current_health,
                is_immune,
                is_pet,
            ));
        }
    }

    // Sort by canonical slot index, NOT entity ID. Across multiple matches in a
    // single process, Bevy reuses despawned entity IDs from the free list — slot 0
    // can end up with a higher entity index than slot 1. Sorting by entity.index()
    // would then put slot 1 first, so kill_target=Some(0) would resolve to the
    // wrong combatant. Pets have slot >= PET_SLOT_BASE (10+), so they sort after
    // primaries naturally. Look up slot per entity from the live combatants query.
    let slot_lookup: std::collections::HashMap<Entity, u8> =
        combatants.iter().map(|(e, c, _, _)| (e, c.slot)).collect();
    team1_combatants.sort_by_key(|(entity, _, _, _, _, _, _, _)| {
        slot_lookup.get(entity).copied().unwrap_or(u8::MAX)
    });
    team2_combatants.sort_by_key(|(entity, _, _, _, _, _, _, _)| {
        slot_lookup.get(entity).copied().unwrap_or(u8::MAX)
    });

    // Build pet-filtered lists for config index lookups (kill_target, cc_target).
    // Config indices are 0-based slot indices into primary combatants only.
    // The full lists (including pets) are kept for nearest-enemy fallback targeting.
    let team1_primary: Vec<_> = team1_combatants
        .iter()
        .filter(|(_, _, _, _, _, _, _, is_pet)| !is_pet)
        .collect();
    let team2_primary: Vec<_> = team2_combatants
        .iter()
        .filter(|(_, _, _, _, _, _, _, is_pet)| !is_pet)
        .collect();

    // The name a Hunter's own Freezing Trap aura carries, from the one
    // definition `trap_system` applies.
    let freezing_trap = super::traps::freezing_trap_aura(Entity::PLACEHOLDER).ability_name;

    // For each combatant, ensure they have a valid target
    for (entity, mut combatant, transform, _) in combatants.iter_mut() {
        if !combatant.is_alive() {
            combatant.target = None;
            combatant.cc_target = None;
            continue;
        }

        // Pets are excluded from acquire_targets entirely (per U1 of the pet
        // engagement plan). Pet target ownership lives in pet_ai_system, which
        // assigns pet.target = owner.target at the top of its per-pet loop.
        // This change is mirror to the existing dispatch-loop skip further down.
        if pet_query.get(entity).is_ok() {
            continue;
        }

        // Snapshot target state BEFORE acquisition so we can emit a trace event
        // when either changes.
        let is_pet_entity = false;
        let prev_target = combatant.target;
        let prev_cc_target = combatant.cc_target;

        // Check if this combatant has Shadow Sight
        let i_have_shadow_sight = shadow_sight_holders.contains(&entity);

        // Get enemy team combatants and target priorities
        let (enemy_combatants, enemy_primary, kill_target_index, cc_target_index) =
            if combatant.team == 1 {
                (
                    &team2_combatants,
                    &team2_primary,
                    config.team1_kill_target,
                    config.team1_cc_target,
                )
            } else {
                (
                    &team1_combatants,
                    &team1_primary,
                    config.team2_kill_target,
                    config.team2_cc_target,
                )
            };

        // Visibility: the shared rule every class AI's view is built from.
        let can_see = |stealthed: bool, enemy_has_shadow_sight: bool| -> bool {
            class_ai::stealth_visible(stealthed, i_have_shadow_sight, enemy_has_shadow_sight)
        };

        // Check if current target is still valid (alive, on enemy team, visible, and not immune)
        let target_valid = combatant
            .target
            .and_then(|target_entity| {
                enemy_combatants
                    .iter()
                    .find(|(e, _, _, _, _, _, _, _)| *e == target_entity)
                    .filter(|(_, _, stealthed, enemy_ss, _, _, immune, _)| {
                        can_see(*stealthed, *enemy_ss) && !immune
                    })
            })
            .is_some();

        // If no valid target, acquire a new one
        if !target_valid {
            // Priority 1: Check if kill target is set, visible, and not immune
            // Use pet-filtered list so config indices map to primary combatant slots
            let kill_target = if let Some(index) = kill_target_index {
                enemy_primary
                    .get(index)
                    .filter(|(_, _, stealthed, enemy_ss, _, _, immune, _)| {
                        can_see(*stealthed, *enemy_ss) && !immune
                    })
                    .map(|(entity, _, _, _, _, _, _, _)| *entity)
            } else {
                None
            };
            // STEP 5'S PLAN CALL WAS CONSUMED HERE, AND IS DELIBERATELY NOT.
            // Four variants were measured at n=100 (gates-open and at-contact
            // picks, nearest-to-melee and healer-first priorities), and EVERY
            // static held call was catastrophic for at least one side of a
            // matchup (-48 to -69pt), while helping its mirror (+25 to +65):
            // each comp wants a different target called, and a call held
            // constant cannot adapt the way per-unit re-acquisition does. The
            // full table lives in the design doc's step-5 amendment. Mid-match
            // switching is the PREREQUISITE for consuming the call — until it
            // exists, `plan.kill_target` stays producer-only (the step-2
            // provable-no-op shape).

            if let Some(priority_target) = kill_target {
                // Use the kill target
                combatant.target = Some(priority_target);
            } else {
                // Priority 2: Fall back to nearest visible, non-immune enemy
                // Prefer non-pet targets; only target pets if no primary enemies are available
                let my_pos = transform.translation;
                let visible_enemies: Vec<_> = enemy_combatants
                    .iter()
                    .filter(|(_, _, stealthed, enemy_ss, _, _, immune, _)| {
                        can_see(*stealthed, *enemy_ss) && !immune
                    })
                    .collect();

                // Try non-pet enemies first, fall back to all visible if none
                let nearest_primary = visible_enemies
                    .iter()
                    .filter(|(_, _, _, _, _, _, _, is_pet)| !is_pet)
                    .min_by(
                        |(_, pos_a, _, _, _, _, _, _), (_, pos_b, _, _, _, _, _, _)| {
                            my_pos
                                .distance(*pos_a)
                                .partial_cmp(&my_pos.distance(*pos_b))
                                .unwrap()
                        },
                    );

                let nearest_enemy = nearest_primary.or_else(|| {
                    visible_enemies.iter().min_by(
                        |(_, pos_a, _, _, _, _, _, _), (_, pos_b, _, _, _, _, _, _)| {
                            my_pos
                                .distance(*pos_a)
                                .partial_cmp(&my_pos.distance(*pos_b))
                                .unwrap()
                        },
                    )
                });

                combatant.target = nearest_enemy.map(|(entity, _, _, _, _, _, _, _)| *entity);
            }
        } else if let Some(index) = kill_target_index {
            // Current target is valid, but check if configured kill target has become
            // available (e.g., Rogue broke stealth) and should take priority —
            // UNLESS a melee target-swap is currently sticky (bucket A, below),
            // which deliberately holds a softer in-melee target for the
            // hysteresis window. Without this gate the re-force would undo the
            // swap every tick (ping-pong).
            let swap_sticky = combatant.class.is_melee()
                && decision_trace.current_sim_time - combatant.last_target_swap_time
                    < movement_config.melee.swap_hysteresis;
            if !swap_sticky {
                if let Some((kt_entity, _, stealthed, enemy_ss, _, _, immune, _)) =
                    enemy_primary.get(index)
                {
                    if can_see(*stealthed, *enemy_ss)
                        && !immune
                        && combatant.target != Some(*kt_entity)
                    {
                        combatant.target = Some(*kt_entity);
                    }
                }
            }
        }

        // ===== Ranged DPS: the team's target focus (AS-389) =====
        // A ranged DPS whose team has moved its focus off a target it could
        // not see takes the focus, unless the player's kill call names a
        // target (which wins). `TeamPlan` only: under `Legacy` no focus is
        // ever classified. Before the Hunter's trap retarget, which still
        // moves it off a healer its own trap holds.
        if kill_target_index.is_none() {
            let team_situation = situations
                .as_deref()
                .and_then(|s| s.teams.get(combatant.team.wrapping_sub(1) as usize));
            let focus = team_situation.and_then(|t| t.target_focus);
            let kill_subject = team_situation.and_then(|t| t.kill_window.map(|w| w.subject));
            let obstacles = map_geometry
                .as_deref()
                .map_or(&[][..], |g| g.volumes.as_slice());
            let find = |e: Entity| enemy_combatants.iter().find(|(x, ..)| *x == e);
            if let Some(next) = class_ai::target_focus::retarget(
                focus,
                entity,
                combatant.class,
                transform.translation,
                combatant.target,
                kill_subject,
                obstacles,
                |e| {
                    find(e).is_some_and(|(_, _, stealthed, enemy_ss, _, _, immune, is_pet)| {
                        can_see(*stealthed, *enemy_ss) && !immune && !is_pet
                    })
                },
                |e| find(e).map(|(_, pos, ..)| *pos),
            ) {
                combatant.target = Some(next);
            }
        }

        // ===== Hunter: off the enemy healer its own Freezing Trap is for =====
        // A Hunter holds fire on an enemy its own Freezing Trap has frozen (the
        // trap breaks on any damage), so staying on a healer it trapped under
        // melee pressure would idle it for the trap's duration. It fights the
        // melee instead (`TrapRetarget`), from the throw — before the trap has
        // sprung — and while the trap holds that healer, however late it
        // sprang. Applied after the kill-target re-force so a configured kill
        // target cannot pull it back, and here rather than in the Hunter's AI
        // so the switch is traced as a target acquisition like any other.
        // Scoped to healers: a trapped non-healer kill target (a stealthed
        // Rogue caught by the opening lane trap) keeps the Hunter on it, since
        // the Hunter's team converges on it the moment the trap ends. Only a
        // Hunter owns a Freezing Trap, so for every other class this block is
        // inert.
        let own_trap_on = |target: Entity| {
            active_auras_map.get(&target).is_some_and(|auras| {
                auras
                    .iter()
                    .any(|a| a.ability_name == freezing_trap && a.caster == Some(entity))
            })
        };
        if let Some(hold) = combatant.trap_retarget {
            let healer_alive = enemy_combatants.iter().any(|(e, ..)| *e == hold.healer);
            if !healer_alive
                || (decision_trace.current_sim_time > hold.spring_by && !own_trap_on(hold.healer))
            {
                combatant.trap_retarget = None;
            }
        }
        let held = combatant.trap_retarget;
        let is_healer = |target: Entity| {
            enemy_combatants
                .iter()
                .any(|(e, _, _, _, class, _, _, is_pet)| {
                    *e == target && !is_pet && class.is_healer()
                })
        };
        if let Some(avoid) = combatant
            .target
            .filter(|t| held.is_some_and(|h| h.healer == *t) || (own_trap_on(*t) && is_healer(*t)))
        {
            let my_pos = transform.translation;
            let fightable = |(e, _, stealthed, enemy_ss, _, _, immune, _): &&(
                Entity,
                Vec3,
                bool,
                bool,
                match_config::CharacterClass,
                f32,
                bool,
                bool,
            )| {
                *e != avoid && !own_trap_on(*e) && can_see(*stealthed, *enemy_ss) && !immune
            };
            let melee = held.and_then(|h| {
                enemy_combatants
                    .iter()
                    .filter(fightable)
                    .find(|(e, ..)| *e == h.melee)
            });
            let nearest = || {
                enemy_combatants
                    .iter()
                    .filter(fightable)
                    .filter(|(.., is_pet)| !is_pet)
                    .min_by(|(_, a, ..), (_, b, ..)| {
                        my_pos.distance(*a).total_cmp(&my_pos.distance(*b))
                    })
            };
            if let Some((next, ..)) = melee.or_else(nearest) {
                combatant.target = Some(*next);
            }
        }

        // ===== Bucket A: melee target-swap =====
        // When a melee has chased its CURRENT target (configured kill target OR
        // a nearest-acquired one — both stick once acquired) persistently out of
        // melee reach and a softer enemy is already in melee, swap to it instead
        // of chasing forever (the #1 "bot tell"). Runs AFTER the re-force above
        // in the same tick, so for the configured case the kill target's
        // intermediate write is never observed; the `swap_sticky` gate on that
        // re-force holds the swap across the hysteresis window (no ping-pong).
        if combatant.class.is_melee() {
            if let Some((cur_entity, cur_pos, _, _, _, cur_health, _, _)) = combatant
                .target
                .and_then(|t| enemy_combatants.iter().find(|(e, ..)| *e == t))
                .copied()
            {
                let my_pos = transform.translation;
                let now = decision_trace.current_sim_time;
                let swap_cfg = &movement_config.melee;

                // A change in the chase focus (re-acquisition, prior swap, the
                // re-force above) restarts the chase clock so each new focus is
                // pursued for the full hysteresis window before a swap is
                // considered.
                if combatant.last_kill_target != Some(cur_entity) {
                    combatant.last_kill_target = Some(cur_entity);
                    combatant.last_target_swap_time = now;
                }

                let out_of_reach = my_pos.distance(cur_pos) > swap_cfg.swap_range;
                let chased_long_enough =
                    now - combatant.last_target_swap_time >= swap_cfg.swap_hysteresis;
                if out_of_reach && chased_long_enough {
                    let candidates = enemy_combatants.iter().filter_map(
                        |(e, pos, st, ss, _, hp, imm, is_pet)| {
                            if *e == cur_entity || *is_pet || *imm || *hp <= 0.0 {
                                return None;
                            }
                            if !can_see(*st, *ss) {
                                return None;
                            }
                            Some((*e, my_pos.distance(*pos), *hp))
                        },
                    );
                    if let Some(softer) = class_ai::select_softer_melee_target(
                        cur_health,
                        candidates,
                        swap_cfg.swap_range,
                        swap_cfg.swap_hp_margin,
                    ) {
                        combatant.target = Some(softer);
                        combatant.last_kill_target = Some(softer);
                        combatant.last_target_swap_time = now;
                    }
                }
            }
        }

        // ===== CC Target Acquisition =====
        // Separate from kill target - use for CC abilities to create outnumbering situations

        // Check if current CC target is still valid (not immune)
        let cc_target_valid = combatant
            .cc_target
            .and_then(|cc_target_entity| {
                enemy_combatants
                    .iter()
                    .find(|(e, _, _, _, _, _, _, _)| *e == cc_target_entity)
                    .filter(|(_, _, stealthed, enemy_ss, _, _, immune, _)| {
                        can_see(*stealthed, *enemy_ss) && !immune
                    })
            })
            .is_some();

        if !cc_target_valid {
            // Priority 1: Use explicitly configured CC target (if visible and not immune)
            // Use pet-filtered list so config indices map to primary combatant slots
            let explicit_cc_target = if let Some(index) = cc_target_index {
                enemy_primary
                    .get(index)
                    .filter(|(_, _, stealthed, enemy_ss, _, _, immune, _)| {
                        can_see(*stealthed, *enemy_ss) && !immune
                    })
                    .map(|(entity, _, _, _, _, _, _, _)| *entity)
            } else {
                None
            };

            if let Some(cc_target) = explicit_cc_target {
                combatant.cc_target = Some(cc_target);
            } else {
                // Priority 2: Use heuristic selection
                // Score: Healer +100, Non-kill-target +50, Higher HP +20
                // Skip already-CC'd targets
                combatant.cc_target = select_cc_target_heuristic(
                    enemy_combatants,
                    combatant.target,
                    &active_auras_map,
                    &can_see,
                );
            }
        } else if let Some(index) = cc_target_index {
            // Current CC target is valid, but check if configured CC target has become
            // available (e.g., broke stealth) and should take priority
            if let Some((cc_entity, _, stealthed, enemy_ss, _, _, immune, _)) =
                enemy_primary.get(index)
            {
                if can_see(*stealthed, *enemy_ss)
                    && !immune
                    && combatant.cc_target != Some(*cc_entity)
                {
                    combatant.cc_target = Some(*cc_entity);
                }
            }
        }

        // Emit a target_acquisition trace event when target or cc_target changed.
        // Pets are not primary actors in target acquisition; skip them entirely
        // to keep the trace focused on combatant-level decisions.
        if !is_pet_entity
            && (combatant.target != prev_target || combatant.cc_target != prev_cc_target)
        {
            use crate::states::play_match::decision_trace::{
                ActorView, CandidateStatus, TargetRejectionReason,
            };
            let my_pos = transform.translation;
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
            let mut tbuilder =
                decision_trace.start_target_acquisition(actor_view, prev_target, prev_cc_target);

            // Populate candidates from the visible enemy set. Chosen = new_target;
            // all other alive non-pet enemies are Rejected{LowerScoreThanChosen}
            // using distance as the score proxy (negated, since nearer = higher
            // priority for primary targeting).
            let chosen = combatant.target;
            let chosen_pos = chosen
                .and_then(|t| {
                    enemy_combatants
                        .iter()
                        .find(|(e, _, _, _, _, _, _, _)| *e == t)
                })
                .map(|(_, p, _, _, _, _, _, _)| *p);
            let chosen_distance = chosen_pos.map(|p| my_pos.distance(p));
            let chosen_score = chosen_distance.map(|d| -(d as i32)).unwrap_or(0);

            for (
                enemy_entity,
                enemy_pos,
                stealthed,
                enemy_ss,
                enemy_class,
                enemy_hp,
                enemy_immune,
                is_pet,
            ) in enemy_combatants.iter()
            {
                if *is_pet || *enemy_hp <= 0.0 {
                    continue;
                }
                let distance = my_pos.distance(*enemy_pos);
                let score = -(distance as i32);
                if Some(*enemy_entity) == chosen {
                    tbuilder.score(
                        *enemy_entity,
                        *enemy_class,
                        score,
                        CandidateStatus::Chosen,
                        None,
                    );
                } else if *enemy_immune {
                    tbuilder.score(
                        *enemy_entity,
                        *enemy_class,
                        score,
                        CandidateStatus::Rejected,
                        Some(TargetRejectionReason::Immune),
                    );
                } else if !can_see(*stealthed, *enemy_ss) {
                    tbuilder.score(
                        *enemy_entity,
                        *enemy_class,
                        score,
                        CandidateStatus::Rejected,
                        Some(TargetRejectionReason::Stealthed),
                    );
                } else {
                    tbuilder.score(
                        *enemy_entity,
                        *enemy_class,
                        score,
                        CandidateStatus::Rejected,
                        Some(TargetRejectionReason::LowerScoreThanChosen {
                            score,
                            chosen_score,
                        }),
                    );
                }
            }
            tbuilder.finish(combatant.target, combatant.cc_target);
        }
    }
}

/// Select the best CC target using heuristics.
/// Priority scoring:
/// 1. Healer: +100 points - highest CC value (UNLESS we're killing the healer)
/// 2. Non-kill-target: +50 points - enables outnumbering
/// 3. Higher HP: +20 points - don't waste CC on dying targets
///
/// Required: Not already CC'd
///
/// Special case: If kill_target is a healer, we INVERT healer priority.
/// When killing the healer, we want to CC the DPS to prevent them from peeling.
///
/// Public (and `#[doc(hidden)]`) only so the decision-property grid picks a
/// scene's CC target with this function rather than a copy of it.
#[doc(hidden)]
pub fn select_cc_target_heuristic(
    enemy_combatants: &[(
        Entity,
        Vec3,
        bool,
        bool,
        match_config::CharacterClass,
        f32,
        bool,
        bool,
    )],
    kill_target: Option<Entity>,
    active_auras_map: &std::collections::HashMap<Entity, Vec<Aura>>,
    can_see: &impl Fn(bool, bool) -> bool,
) -> Option<Entity> {
    // Check if kill target is a healer - if so, we invert healer CC priority
    let killing_healer = kill_target
        .and_then(|kt| {
            enemy_combatants
                .iter()
                .find(|(e, _, _, _, _, _, _, _)| *e == kt)
                .map(|(_, _, _, _, class, _, _, _)| class.is_healer())
        })
        .unwrap_or(false);

    // Filter to visible, non-CC'd, non-immune enemies and score them
    // Pets are excluded — CC on a pet is wasted
    let mut scored_targets: Vec<(Entity, i32)> = enemy_combatants
        .iter()
        .filter(|(_, _, stealthed, enemy_ss, _, _, immune, is_pet)| {
            can_see(*stealthed, *enemy_ss) && !immune && !is_pet
        })
        .filter(|(entity, _, _, _, _, _, _, _)| !is_entity_ccd(*entity, active_auras_map))
        .map(|(entity, _, _, _, class, current_health, _, _)| {
            let mut score = 0i32;
            let is_healer = class.is_healer();

            // Healer/DPS priority depends on who we're killing
            if killing_healer {
                // Killing healer -> CC the DPS to prevent peel
                if !is_healer {
                    score += 100;
                }
            } else {
                // Killing DPS -> CC the healer to prevent healing
                if is_healer {
                    score += 100;
                }
            }

            // Non-kill-target bonus (enables 2v1 situations)
            if kill_target != Some(*entity) {
                score += 50;
            }

            // Higher HP bonus (don't waste CC on dying targets)
            // Scale 0-20 based on health percentage (approximating with raw health)
            // Most classes have 150-200 HP, so 200 HP = 20 points
            score += (*current_health as i32 / 10).min(20);

            (*entity, score)
        })
        .collect();

    // Sort by score descending, then by entity index for determinism
    scored_targets
        .sort_by(|(e1, s1), (e2, s2)| s2.cmp(s1).then_with(|| e1.index().cmp(&e2.index())));

    scored_targets.first().map(|(entity, _)| *entity)
}

/// Per-frame inputs every class decision may read, built once by
/// `decide_abilities` (or by a [`Scene`](class_ai::scene::Scene)).
#[derive(Clone, Copy)]
pub struct DispatchFrame<'a> {
    pub movement_config: &'a crate::states::play_match::movement_config::MovementConfig,
    /// Movement pre-passes (postures, the tempo reset, the Flare walk) run
    /// only once the gates are open.
    pub gates_opened: bool,
    /// The posture clock, `Time::elapsed_secs`.
    pub now: f32,
    pub team1_kill_target: Option<usize>,
    pub team2_kill_target: Option<usize>,
    /// Per-Shaman live totem durations, indexed by `TotemElement::index()`.
    pub totem_durations: &'a std::collections::BTreeMap<Entity, [f32; 4]>,
    /// Per-Hunter Freezing Traps that have not sprung.
    pub own_freezing_traps:
        &'a std::collections::BTreeMap<Entity, Vec<class_ai::hunter_dip::LiveTrap>>,
    /// Every lit Flare with its owner's team, in entity order.
    pub lit_flares: &'a [(u8, class_ai::rogue_flare::LitFlare)],
}

/// The accumulators one frame's dispatch shares across units: attacks queued
/// to land after the loop, the same-frame buff guards, and the instant CC
/// earlier units landed this frame.
#[derive(Default)]
pub struct DispatchAccumulators {
    pub instant_attacks: Vec<class_ai::QueuedInstantAttack>,
    pub shielded_this_frame: std::collections::HashSet<Entity>,
    pub fortified_this_frame: std::collections::HashSet<Entity>,
    pub battle_shouted_this_frame: std::collections::HashSet<Entity>,
    pub paladin_aura_this_frame: std::collections::HashSet<Entity>,
    pub marked_this_frame: std::collections::HashSet<Entity>,
    pub frost_nova_damage: Vec<class_ai::QueuedAoeDamage>,
    pub same_frame_cc_queue: Vec<(Entity, Aura)>,
}

/// One unit's movement-AI state as `decide_abilities`' posture query yields
/// it: healer posture, kiter posture, standing directive, Warrior tempo reset,
/// Rogue Flare walk.
pub type UnitPostures<'a> = (
    Option<Mut<'a, HealerPosture>>,
    Option<Mut<'a, KitePosture>>,
    Option<&'a MovementDirective>,
    Option<Mut<'a, MeleeResetState>>,
    Option<Mut<'a, RogueFlareState>>,
);

/// Where a decision's side effects go.
pub struct DispatchIo<'r, 'w, 's> {
    pub commands: &'r mut Commands<'w, 's>,
    pub combat_log: &'r mut CombatLog,
    pub game_rng: &'r mut GameRng,
    pub decision_trace: &'r mut crate::states::play_match::decision_trace::DecisionTrace,
}

/// The other team of a two-team match.
fn enemy_team_of(team: u8) -> u8 {
    if team == 1 {
        2
    } else {
        1
    }
}

/// What an INCAPACITATED unit may still do this tick: break free. The ONE
/// entry point for a held unit, as [`dispatch_class_decision`] is for a free
/// one: `decide_abilities` calls it for every unit held by a stun, fear,
/// polymorph or incapacitate (a Cyclone excepted), and a
/// [`Scene`](class_ai::scene::Scene) calls it for a held unit under test.
///
/// In order: an on-use item's crowd-control break (any class wearing one,
/// `class_ai::on_use`), then the class's own (the Paladin's Divine Shield,
/// the Warrior's Berserker Rage). Each arm owns its trace event, because the
/// class decision never runs this frame. `enemy_kill_target` is the enemy
/// team's `kill_window` subject — the unit their burst is on. Returns whether
/// the unit acted; the break lands next frame either way.
#[allow(clippy::too_many_arguments)]
pub fn dispatch_while_held(
    io: DispatchIo<'_, '_, '_>,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &class_ai::CombatContext,
    enemy_kill_target: Option<Entity>,
) -> bool {
    let DispatchIo {
        commands,
        combat_log,
        game_rng: _,
        decision_trace,
    } = io;

    if !combatant.on_use_items.is_empty()
        && class_ai::on_use::try_cc_break_while_held(
            commands,
            decision_trace,
            entity,
            combatant,
            ctx,
            enemy_kill_target,
        )
    {
        return true; // OnUsePending spawned — the hold breaks next frame
    }

    // Paladin-specific: Divine Shield can be used while incapacitated.
    // Because the normal dispatch never runs this frame, we own a builder
    // here so the Divine-Shield-while-CC decision still produces a trace
    // event when the predicates fire.
    if combatant.class == match_config::CharacterClass::Paladin {
        let actor_view = ctx
            .self_info()
            .map(crate::states::play_match::decision_trace::ActorView::from_info);
        let Some(av) = actor_view else {
            return false;
        };
        let mut builder = decision_trace
            .start_ability_decision(av, None)
            .with_situation(ctx.situation());
        let book = combatant.book().clone();
        let result = class_ai::paladin::try_divine_shield_while_cc(
            commands,
            combat_log,
            &book,
            entity,
            combatant,
            my_pos,
            auras,
            ctx,
            &mut builder,
        );
        builder.finish();
        return result; // DivineShieldPending spawned — CC will be purged next frame
    }

    // Warrior-specific: Berserker Rage can be used while feared (TBC rule —
    // breaks the fear + grants fear immunity; horror/stun/poly stay locked).
    // Same builder-ownership shape as the Paladin Divine Shield arm above.
    if combatant.class == match_config::CharacterClass::Warrior {
        let actor_view = ctx
            .self_info()
            .map(crate::states::play_match::decision_trace::ActorView::from_info);
        let Some(av) = actor_view else {
            return false;
        };
        let mut builder = decision_trace
            .start_ability_decision(av, None)
            .with_situation(ctx.situation());
        let book = combatant.book().clone();
        let result = class_ai::warrior::try_berserker_rage_while_cc(
            commands,
            combat_log,
            &book,
            entity,
            combatant,
            my_pos,
            auras,
            ctx,
            &mut builder,
        );
        builder.finish();
        return result; // BerserkerRagePending spawned — fear breaks next frame
    }

    false
}

/// One unit's whole class decision for one tick: its movement pre-pass (once
/// the gates are open), then its class's `decide_<class>_action`. The ONE
/// entry point: `decide_abilities` calls it for every unit it dispatches, and
/// a [`Scene`](class_ai::scene::Scene) calls it for the unit under test, so a
/// scene cannot exercise a path a match never takes.
///
/// The match is exhaustive over `CharacterClass`: a new class fails the build
/// here until it has an arm. Each arm names the per-class accumulators it uses
/// rather than threading a bag through every class. Returns whether the unit
/// acted.
#[allow(clippy::too_many_arguments)]
pub fn dispatch_class_decision(
    io: DispatchIo<'_, '_, '_>,
    frame: &DispatchFrame,
    acc: &mut DispatchAccumulators,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &class_ai::CombatContext,
    posture: Option<UnitPostures<'_>>,
) -> bool {
    let DispatchIo {
        commands,
        combat_log,
        game_rng,
        decision_trace,
    } = io;
    let DispatchFrame {
        movement_config,
        gates_opened,
        now,
        team1_kill_target,
        team2_kill_target,
        totem_durations,
        own_freezing_traps,
        lit_flares,
    } = *frame;
    let DispatchAccumulators {
        instant_attacks,
        shielded_this_frame,
        fortified_this_frame,
        battle_shouted_this_frame,
        paladin_aura_this_frame,
        marked_this_frame,
        frost_nova_damage,
        same_frame_cc_queue,
    } = acc;
    // The unit decides from its OWN book. Cloned (a shared handle) so the
    // configs it reads stay borrowable while the unit itself is mutated.
    let book = combatant.book().clone();
    let abilities = &book;
    match combatant.class {
        match_config::CharacterClass::Mage => {
            // Posture evaluation (ENGAGE/KITE, U5) runs BEFORE the ability
            // pass and OUTSIDE decide_mage_action's GCD short-circuit so the
            // directive refreshes during a cast. Gated on gates_opened: no
            // pre-match directives/trace. Casting Mages are excluded by the
            // `combatants` query filter, so KITE exit can lag one GCD — an
            // accepted pilot simplification.
            if gates_opened {
                if let Some((_healer, mage_posture, directive, _reset, _flare)) = posture {
                    // Mage: aura-gated KITE (a melee enemy it rooted/slowed).
                    let cfg = &movement_config.mage;
                    let entry = class_ai::dps_postures::mage_kite_entry(ctx, entity, my_pos);
                    let sustain = class_ai::dps_postures::mage_kite_sustain(
                        ctx,
                        entity,
                        my_pos,
                        cfg.range_band_max,
                    );
                    // OOM wand-pull gate: pull to wand range once the Mage
                    // can't afford a Frostbolt, so its wand auto-attack fires
                    // instead of idling the mana refractory. Cost read from
                    // config, never hardcoded.
                    let wand_gate = Some(class_ai::dps_postures::WandPullGate {
                        current_mana: combatant.current_mana,
                        nuke_cost: abilities.get_unchecked(&AbilityType::Frostbolt).mana_cost,
                    });
                    class_ai::dps_postures::evaluate_dps_posture(
                        commands,
                        entity,
                        my_pos,
                        combatant.target,
                        ctx,
                        mage_posture.map(bevy::prelude::Mut::into_inner),
                        directive,
                        cfg,
                        movement_config.shared.heal_range,
                        entry,
                        sustain,
                        wand_gate,
                        // The Mage has no trap.
                        None,
                        now,
                        decision_trace,
                    );
                }
            }
            class_ai::mage::decide_mage_action(
                commands,
                combat_log,
                game_rng,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                ctx,
                frost_nova_damage,
                same_frame_cc_queue,
                decision_trace,
            )
        }
        match_config::CharacterClass::Priest => {
            // Posture evaluation (healer movement AI, U6/U7) runs BEFORE
            // the ability pass and OUTSIDE the GCD short-circuit inside
            // decide_priest_action — directives must refresh while on GCD
            // (the GCD locks casts, not legs). Gated on gates_opened: no
            // pre-match directives or trace events. Casting Priests never
            // get here (query excludes CastingState/ChannelingState), so
            // posture movement lives in cast gaps only (R12).
            //
            // `escape_defer` is `Some(urgency_hp_threshold)` while an
            // ESCAPE window is live (U7 cast-vs-move urgency): the heal
            // ladder defers non-critical movement-locking casts for the
            // window.
            let mut plan = class_ai::priest::PriestMovementPlan::default();
            if gates_opened {
                if let Some((healer_posture, _mage, directive, _reset, _flare)) = posture {
                    plan = class_ai::priest::evaluate_priest_posture(
                        commands,
                        entity,
                        combatant,
                        my_pos,
                        ctx,
                        abilities,
                        auras,
                        healer_posture.map(bevy::prelude::Mut::into_inner),
                        directive,
                        movement_config,
                        now,
                        decision_trace,
                    );
                }
            }
            class_ai::priest::decide_priest_action(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                ctx,
                shielded_this_frame,
                fortified_this_frame,
                &plan,
                movement_config,
                same_frame_cc_queue,
                decision_trace,
            )
        }
        match_config::CharacterClass::Warrior => {
            // Tempo-reset movement pre-pass — mirrors the Mage arm:
            // runs BEFORE the ability pass and OUTSIDE decide_warrior_action's
            // GCD short-circuit (legs aren't on the GCD), gated on
            // gates_opened, never for casting/CC'd warriors (query excludes
            // CastingState; move_to_target ignores the directive under CC).
            if gates_opened {
                if let Some((_healer, _mage, directive, reset, _flare)) = posture {
                    class_ai::warrior::evaluate_warrior_reset(
                        commands,
                        entity,
                        combatant,
                        my_pos,
                        auras,
                        ctx,
                        reset.map(bevy::prelude::Mut::into_inner),
                        directive,
                        &movement_config.melee,
                        now,
                        decision_trace,
                    );
                }
            }
            class_ai::warrior::decide_warrior_action(
                commands,
                combat_log,
                game_rng,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                ctx,
                instant_attacks,
                battle_shouted_this_frame,
                decision_trace,
            )
        }
        match_config::CharacterClass::Rogue => {
            // Flare pre-pass (AS-185): a stealthed Rogue plays around a lit
            // enemy Flare it can see. Runs before the opener so the walk it
            // sets is this frame's; gated on gates_opened like the other
            // movement pre-passes.
            if gates_opened {
                if let Some((_healer, _mage, _directive, _reset, flare_state)) = posture {
                    let current = flare_state.as_deref().copied();
                    let next = class_ai::rogue_flare::evaluate_rogue_flare(
                        commands,
                        combat_log,
                        entity,
                        combatant,
                        my_pos,
                        ctx,
                        lit_flares,
                        current,
                        now,
                        decision_trace,
                    );
                    // Written only when it changed, so an idle Rogue's
                    // state is never marked changed.
                    match flare_state {
                        Some(mut state) => {
                            state.set_if_neq(next);
                        }
                        None if RogueFlareState::needs_insert(None, &next) => {
                            commands.entity(entity).try_insert(next);
                        }
                        None => {}
                    }
                }
            }
            class_ai::rogue::decide_rogue_action(
                commands,
                combat_log,
                game_rng,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                ctx,
                instant_attacks,
                same_frame_cc_queue,
                decision_trace,
            )
        }
        match_config::CharacterClass::Warlock => {
            // The Hunt play (AS-394): a Warlock holding a station round a
            // lone enemy healer's pillar walks to it and holds it, in place of
            // its pursuit. Gated on gates_opened, as every posture is.
            if gates_opened {
                if let (Some((_healer, _kite, directive, _reset, _flare)), Some(station)) =
                    (posture, ctx.hunt_station())
                {
                    class_ai::hunt::walk_to_station(
                        commands,
                        entity,
                        my_pos,
                        station,
                        directive,
                        crate::states::play_match::decision_trace::Posture::Engage,
                        movement_config.shared.commit_window,
                        movement_config.shared.directive_ttl,
                        now,
                        ctx,
                        decision_trace,
                    );
                }
            }
            class_ai::warlock::decide_warlock_action(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                ctx,
                movement_config,
                decision_trace,
            )
        }
        // Shaman: full healer-posture dispatch (U6). Mirrors the Priest arm
        // — posture evaluation (FREE/PRESSURED/ESCAPE) runs BEFORE the
        // ability pass and OUTSIDE the GCD short-circuit so directives
        // refresh while on GCD; gated on gates_opened; never for casting
        // Shamans (query excludes CastingState/ChannelingState — R12). The
        // Shaman reuses the shared HealerPosture query slot. The plan's
        // `escape_defer` makes the rotation defer non-critical casts during
        // an ESCAPE window. Per-element totem durations come from the live
        // Totem query snapshot above.
        match_config::CharacterClass::Shaman => {
            let durations = totem_durations.get(&entity).copied().unwrap_or([0.0; 4]);
            let mut plan = class_ai::caster_healer_posture::CasterHealerPlan::default();
            if gates_opened {
                if let Some((healer_posture, _mage, directive, _reset, _flare)) = posture {
                    plan = class_ai::caster_healer_posture::evaluate_caster_healer_posture(
                        commands,
                        entity,
                        combatant,
                        my_pos,
                        ctx,
                        healer_posture.map(bevy::prelude::Mut::into_inner),
                        directive,
                        movement_config,
                        &movement_config.shaman,
                        // The Shaman has no shapeshift.
                        false,
                        now,
                        decision_trace,
                    );
                }
            }
            class_ai::shaman::decide_shaman_action(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                ctx,
                &durations,
                &plan,
                movement_config,
                decision_trace,
            )
        }
        // Druid: the Shaman's caster-healer posture machine on its own
        // `druid:` block, then a rotation of instants. The posture's
        // ESCAPE deferral is not threaded into the rotation — every heal
        // the Druid has is an instant, so there is nothing to defer. The
        // medic walk's teammate is — the `Legacy` chase's, or the `TeamPlan`
        // solve's walk to a dying teammate: while it is out of reach the
        // rotation holds damage and utility for the heal (`class_ai::druid`).
        match_config::CharacterClass::Druid => {
            let mut plan = class_ai::caster_healer_posture::CasterHealerPlan::default();
            if gates_opened {
                if let Some((healer_posture, _mage, directive, _reset, _flare)) = posture {
                    plan = class_ai::caster_healer_posture::evaluate_caster_healer_posture(
                        commands,
                        entity,
                        combatant,
                        my_pos,
                        ctx,
                        healer_posture.map(bevy::prelude::Mut::into_inner),
                        directive,
                        movement_config,
                        &movement_config.druid,
                        // Travel Form: a shifted Druid runs (ESCAPE).
                        auras.is_some_and(ActiveAuras::is_shapeshifted),
                        now,
                        decision_trace,
                    );
                }
            }
            class_ai::druid::decide_druid_action(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                ctx,
                movement_config,
                plan.pressured,
                plan.medic_walk,
                gates_opened,
                marked_this_frame,
                decision_trace,
            )
        }
        match_config::CharacterClass::Paladin => {
            // Posture evaluation (healer movement AI, U8) — mirrors the
            // Priest arm: runs BEFORE the ability pass and OUTSIDE the
            // GCD short-circuit, gated on gates_opened, and never for
            // casting Paladins (query excludes CastingState — R12).
            //
            // The returned plan carries the ESCAPE/DIP cast deferral
            // (Flash of Light / Holy Light are skipped for targets above
            // the urgency threshold while a window or dip is live) and
            // the Hammer of Justice gate (reservation for the
            // enemy-healer dip; `DipCast` on dip arrival).
            let mut plan = class_ai::paladin::PaladinMovementPlan::default();
            if gates_opened {
                if let Some((healer_posture, _mage, directive, _reset, _flare)) = posture {
                    plan = class_ai::paladin::evaluate_paladin_posture(
                        commands,
                        abilities,
                        entity,
                        combatant,
                        my_pos,
                        auras,
                        ctx,
                        healer_posture.map(bevy::prelude::Mut::into_inner),
                        directive,
                        movement_config,
                        now,
                        decision_trace,
                    );
                }
            }
            class_ai::paladin::decide_paladin_action(
                commands,
                combat_log,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                ctx,
                paladin_aura_this_frame,
                same_frame_cc_queue,
                &plan,
                decision_trace,
            )
        }
        match_config::CharacterClass::Hunter => {
            // Hunter movement is the proximity-gated ENGAGE/KITE machine
            // (the Mage's aura-gating doesn't fit — the Hunter has no
            // reliable self-root and kites on melee proximity). KITE only
            // when a melee-DPS threat (Warrior/Rogue) is within closing
            // range — `melee_within` excludes ranged classes so the Hunter
            // holds and shoots a caster instead of fleeing it. The ability
            // AI keeps its own dead/closing/safe band priorities. Same
            // gates-open + casting-excluded contract as the Mage.
            // Freezing Trap DIP (committed walk to set a trap on the enemy
            // healer) is evaluated BEFORE the ENGAGE/KITE machine. While the
            // dip owns movement (walking toward / arrived at the healer) the
            // KITE evaluation is skipped so its kite vector doesn't overwrite
            // the dip's Entity directive. Outside a dip it returns Rotation
            // and KITE runs unchanged.
            let mut dip_plan = class_ai::hunter_dip::HunterDipPlan::Rotation;
            if gates_opened {
                if let Some((_healer, mut kite_posture, directive, _reset, _flare)) = posture {
                    let cfg = &movement_config.hunter;
                    dip_plan = class_ai::hunter_dip::evaluate_hunter_dip(
                        commands,
                        abilities,
                        entity,
                        combatant,
                        my_pos,
                        auras,
                        ctx,
                        kite_posture.as_deref_mut(),
                        directive,
                        cfg,
                        now,
                        decision_trace,
                    );
                    if !dip_plan.owns_movement() {
                        let entry = class_ai::dps_postures::melee_within(
                            ctx,
                            entity,
                            my_pos,
                            cfg.kite_entry_radius,
                        );
                        let sustain = class_ai::dps_postures::melee_within(
                            ctx,
                            entity,
                            my_pos,
                            cfg.kite_sustain_radius,
                        );
                        // While Freezing Trap is ready and an enemy healer
                        // would be caught cleanly, the kite bends to keep
                        // it within throw range; on cooldown, unchanged.
                        let trap_setup = class_ai::hunter_dip::trap_setup(
                            abilities, entity, combatant, my_pos, auras, ctx,
                        );
                        class_ai::dps_postures::evaluate_dps_posture(
                            commands,
                            entity,
                            my_pos,
                            combatant.target,
                            ctx,
                            kite_posture.map(bevy::prelude::Mut::into_inner),
                            directive,
                            cfg,
                            movement_config.shared.heal_range,
                            entry,
                            sustain,
                            // No wand gate: the Hunter has no wand and shoots,
                            // it doesn't fall back to a wand when OOM.
                            None,
                            trap_setup,
                            now,
                            decision_trace,
                        );
                    }
                }
            }
            let kill_target_index = match combatant.team {
                1 => team1_kill_target,
                _ => team2_kill_target,
            };
            class_ai::hunter::decide_hunter_action(
                commands,
                combat_log,
                game_rng,
                abilities,
                entity,
                combatant,
                my_pos,
                auras,
                ctx,
                instant_attacks,
                dip_plan,
                &movement_config.hunter.weights,
                own_freezing_traps
                    .get(&entity)
                    .map_or(&[][..], Vec::as_slice),
                kill_target_index,
                decision_trace,
            )
        }
    }
}

/// Check if an entity is currently CC'd (Stun, Fear, Root, or Polymorph).
fn is_entity_ccd(
    entity: Entity,
    active_auras_map: &std::collections::HashMap<Entity, Vec<Aura>>,
) -> bool {
    active_auras_map
        .get(&entity)
        .map(|auras| {
            auras.iter().any(|a| {
                matches!(
                    a.effect_type,
                    AuraType::Stun | AuraType::Fear | AuraType::Root | AuraType::Polymorph
                )
            })
        })
        .unwrap_or(false)
}
pub fn decide_abilities(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut game_rng: ResMut<GameRng>,
    countdown: Res<MatchCountdown>,
    time: Res<Time>,
    movement_config: Res<crate::states::play_match::movement_config::MovementConfig>,
    mut combatants: Query<
        (
            Entity,
            &mut Combatant,
            &Transform,
            Option<&mut ActiveAuras>,
            Option<&ChargingState>,
            Option<&DisengagingState>,
        ),
        (Without<CastingState>, Without<ChannelingState>),
    >,
    casting_auras: Query<
        (
            Entity,
            &Combatant,
            &Transform,
            Option<&ActiveAuras>,
            &CastingState,
        ),
        With<CastingState>,
    >,
    channeling_auras: Query<
        (
            Entity,
            &Combatant,
            &Transform,
            Option<&ActiveAuras>,
            &ChannelingState,
        ),
        (With<ChannelingState>, Without<CastingState>),
    >,
    dr_tracker_query: Query<(Entity, &DRTracker)>,
    // Posture state + standing directive read-back (movement AI). One query
    // for the healer (HealerPosture), DPS-kiter (KitePosture), and Warrior
    // tempo-reset (MeleeResetState) states plus the shared directive — kept
    // single to stay under Bevy's 16 system-param limit. Disjoint from
    // `combatants` component-wise, so the two coexist.
    mut posture_movement: Query<(
        Option<&mut HealerPosture>,
        Option<&mut KitePosture>,
        Option<&MovementDirective>,
        Option<&mut MeleeResetState>,
        Option<&mut RogueFlareState>,
    )>,
    mut fct_states: Query<&mut FloatingTextState>,
    extras: AbilityDispatchExtras,
    pet_query: Query<&Pet>,
    mut decision_trace: ResMut<crate::states::play_match::decision_trace::DecisionTrace>,
) {
    // Don't cast abilities during victory celebration
    if extras.celebration.is_some() {
        return;
    }

    // Per-Shaman snapshot of live totem durations, keyed by owner Shaman. Each
    // entry is `[max duration_remaining; 4]` indexed by `TotemElement::index()`
    // (0.0 = that element's totem is absent). The Shaman AI reads its own entry
    // to decide which totems to (re)drop. Freshly spawned totems appear next
    // frame; the cast cooldown covers that gap so we never double-drop.
    let mut totem_durations: std::collections::BTreeMap<Entity, [f32; 4]> =
        std::collections::BTreeMap::new();
    for totem in extras.totems.iter() {
        let slot =
            &mut totem_durations.entry(totem.owner).or_insert([0.0; 4])[totem.element.index()];
        if totem.duration_remaining > *slot {
            *slot = totem.duration_remaining;
        }
    }

    // Per-Hunter list of its own Freezing Traps that have not sprung — landed
    // (arming or armed) or still in flight — with the seconds until each arms.
    let mut own_freezing_traps: std::collections::BTreeMap<
        Entity,
        Vec<class_ai::hunter_dip::LiveTrap>,
    > = std::collections::BTreeMap::new();
    for (trap, transform) in extras.traps.iter() {
        if trap.trap_type == TrapType::Freezing && !trap.triggered {
            own_freezing_traps.entry(trap.owner).or_default().push(
                class_ai::hunter_dip::LiveTrap {
                    position: transform.translation,
                    armed_in: trap.arm_timer.max(0.0),
                },
            );
        }
    }
    for launch in extras.trap_launches.iter() {
        if launch.trap_type == TrapType::Freezing {
            let flight =
                (launch.total_distance - launch.distance_traveled).max(0.0) / TRAP_LAUNCH_SPEED;
            own_freezing_traps.entry(launch.owner).or_default().push(
                class_ai::hunter_dip::LiveTrap {
                    position: launch.landing_position,
                    armed_in: flight + TRAP_ARM_DELAY,
                },
            );
        }
    }

    // Every lit Flare with its owner's team, in entity order, for the stealthed
    // Rogue's pre-pass. Empty in any match without a Hunter that has lit one.
    let mut lit_flares: Vec<(Entity, u8, class_ai::rogue_flare::LitFlare)> = extras
        .flares
        .iter()
        .map(|(e, zone, transform)| {
            (
                e,
                zone.owner_team,
                class_ai::rogue_flare::LitFlare {
                    center: Vec2::new(transform.translation.x, transform.translation.z),
                    radius: zone.radius,
                    remaining: zone.duration_remaining,
                },
            )
        })
        .collect();
    lit_flares.sort_by_key(|(e, _, _)| *e);
    let lit_flares: Vec<(u8, class_ai::rogue_flare::LitFlare)> = lit_flares
        .into_iter()
        .map(|(_, team, f)| (team, f))
        .collect();

    // CombatantInfo is a per-frame snapshot. Mutations to Combatant components
    // during class AI dispatch are not reflected in other entities' views.
    // Safe because each entity is dispatched at most once per frame.
    //
    // The snapshot's `active_auras` and `dr_trackers` are mutated in-place by
    // `snapshot.reflect_instant_cc` below as instant-CC abilities land, so
    // later combatants in this frame's dispatch see those CCs immediately.
    let mut snapshot = class_ai::combat_snapshot::CombatSnapshot::build(
        &combatants,
        &casting_auras,
        &channeling_auras,
        &dr_tracker_query,
        &pet_query,
        &extras.map_geometry.volumes,
        extras.map_geometry.bounds,
        // Resolve to the ACTING unit's team, so `ctx.ai_profile` means "the
        // implementation I run under" and every existing gate becomes per-team
        // for free.
        extras.ai_profile.map(|p| *p).unwrap_or_default(),
    );

    // The per-frame accumulators the dispatch shares across units:
    // - instant attacks (Ambush, Sinister Strike, Mortal Strike) and Frost Nova
    //   damage, queued to land after the loop;
    // - the targets shielded / fortified / Battle-Shouted / given a Paladin aura
    //   / sent a Mark of the Wild THIS FRAME, so a second caster of the same
    //   buff does not spend a cast on one that lands next frame (AuraPending is
    //   not processed until Phase 1);
    // - the instant-CC auras earlier combatants landed this frame. Each class AI
    //   that lands an instant CC (Cheap Shot, Kidney Shot, Hammer of Justice,
    //   Frost Nova root) pushes the target + aura here. Before the next
    //   combatant's turn the queue drains into the snapshot via
    //   `auras::reflect_instant_cc_in_snapshot`, so the target's own self-check
    //   and any downstream `CombatContext::is_ccd` lookup see the CC in the same
    //   frame — closing the "Cheap Shot then Kick on same target same frame"
    //   wasted-interrupt gap.
    let mut acc = DispatchAccumulators::default();

    let frame = DispatchFrame {
        movement_config: &movement_config,
        gates_opened: countdown.gates_opened,
        now: time.elapsed_secs(),
        team1_kill_target: extras.config.as_ref().and_then(|c| c.team1_kill_target),
        team2_kill_target: extras.config.as_ref().and_then(|c| c.team2_kill_target),
        totem_durations: &totem_durations,
        own_freezing_traps: &own_freezing_traps,
        lit_flares: &lit_flares,
    };

    // Each team's situation (`situation.rs`), classified from THIS snapshot —
    // the one every unit below decides from — and re-classified whenever an
    // earlier unit's instant CC changes it. A scene without the resource gets
    // the facts, with no history.
    let situation_cfg = SituationConfig {
        withheld: extras
            .withheld_plays
            .as_deref()
            .copied()
            .unwrap_or_default(),
        ..SituationConfig::from_movement(&movement_config)
    };
    let clock = MatchClock {
        now: decision_trace.current_sim_time,
        gates_opened: countdown.gates_opened,
        since_gates: extras
            .dampening
            .as_ref()
            .map_or(0.0, |d| d.time_since_gates),
    };
    let mut situations = extras.situations.as_deref().cloned().unwrap_or_default();
    update_situations(
        &mut situations,
        &snapshot,
        &clock,
        &situation_cfg,
        &mut decision_trace,
    );

    // Charging/Disengaging combatants are CC-immune in the real application path
    // (apply_pending_auras rejects the aura with "Immune (charging)"). Mirror that
    // here so the snapshot reflection doesn't show a phantom CC on an unstoppable
    // target — without this, a Warrior screamed mid-Charge saw a one-frame phantom
    // fear and wasted Berserker Rage's cooldown answering it.
    let unstoppable: std::collections::BTreeSet<Entity> = combatants
        .iter()
        .filter(|(_, _, _, _, charging, disengaging)| charging.is_some() || disengaging.is_some())
        .map(|(e, _, _, _, _, _)| e)
        .collect();

    for (entity, mut combatant, transform, auras, _, _) in combatants.iter_mut() {
        // Flush pending same-frame CCs into the per-frame snapshot BEFORE any checks
        // or decisions for this combatant, so that stuns/roots landed earlier this frame
        // are visible to this combatant's incapacitation check and its CombatContext.
        let mut reflected = false;
        for (cc_target, cc_aura) in acc.same_frame_cc_queue.drain(..) {
            let is_cc = matches!(
                cc_aura.effect_type,
                AuraType::Fear
                    | AuraType::Stun
                    | AuraType::Root
                    | AuraType::Polymorph
                    | AuraType::Incapacitate
            );
            if is_cc && unstoppable.contains(&cc_target) {
                continue; // real path will reject it as "Immune (charging)"
            }
            snapshot.reflect_instant_cc(cc_target, &cc_aura);
            reflected = true;
        }
        // A CC an earlier unit just landed can lock the enemy healer for every
        // unit after it this frame.
        if reflected {
            update_situations(
                &mut situations,
                &snapshot,
                &clock,
                &situation_cfg,
                &mut decision_trace,
            );
        }
        let situation = situations.for_team(combatant.team);

        if !combatant.is_alive() {
            continue;
        }

        // Pets use pet_ai_system for their decisions, skip class AI dispatch
        if pet_query.get(entity).is_ok() {
            continue;
        }

        // WoW Mechanic: Cannot use abilities while stunned, feared, or polymorphed.
        // Read from the per-frame snapshot (not live `auras`) so that instant CCs
        // landed earlier this frame by other class AIs are observed here.
        let is_incapacitated = snapshot
            .active_auras
            .get(&entity)
            .map(|auras_slice| {
                auras_slice
                    .iter()
                    .any(|a| super::utils::is_incapacitating(&a.effect_type))
            })
            .unwrap_or(false);
        // Nothing is used from inside a Cyclone — not even the CC-break
        // cooldowns below, which answer the CC a Cyclone is not.
        let is_cycloned = snapshot
            .active_auras
            .get(&entity)
            .is_some_and(|auras_slice| {
                auras_slice
                    .iter()
                    .any(|a| a.effect_type == AuraType::Cyclone)
            });

        // An incapacitated unit runs only what may be used from inside the
        // hold — never from inside a Cyclone, which is not a hold anything
        // answers.
        if is_incapacitated {
            if !is_cycloned {
                let held_ctx = snapshot.context_for(entity).with_situation(situation);
                // The enemy team's kill window, when this unit wears something
                // that asks whether it is the one being burst.
                let enemy_kill_target = if combatant.on_use_items.is_empty() {
                    None
                } else {
                    situations
                        .for_team(enemy_team_of(combatant.team))
                        .kill_window
                        .map(|w| w.subject)
                };
                dispatch_while_held(
                    DispatchIo {
                        commands: &mut commands,
                        combat_log: &mut combat_log,
                        game_rng: &mut game_rng,
                        decision_trace: &mut decision_trace,
                    },
                    entity,
                    &mut combatant,
                    transform.translation,
                    auras.as_deref(),
                    &held_ctx,
                    enemy_kill_target,
                );
            }
            continue; // Still incapacitated, or the break lands next frame
        }

        let my_pos = transform.translation;

        let ctx = snapshot.context_for(entity).with_situation(situation);

        let acted = dispatch_class_decision(
            DispatchIo {
                commands: &mut commands,
                combat_log: &mut combat_log,
                game_rng: &mut game_rng,
                decision_trace: &mut decision_trace,
            },
            &frame,
            &mut acc,
            entity,
            &mut combatant,
            my_pos,
            auras.as_deref(),
            &ctx,
            posture_movement.get_mut(entity).ok(),
        );
        if acted {
            continue;
        }
    }
    if let Some(mut stored) = extras.situations {
        stored.set_if_neq(situations);
    }

    // Process queued instant attacks (Ambush, Sinister Strike, Mortal Strike)
    for atk in acc.instant_attacks {
        let class_ai::QueuedInstantAttack {
            attacker: attacker_entity,
            target: target_entity,
            damage,
            attacker_team,
            attacker_slot,
            attacker_class,
            ability,
            is_crit,
        } = atk;
        // The attacker's own book: it was in this frame's snapshot to queue
        // the attack at all.
        let ability_def = snapshot
            .combatants
            .get(&attacker_entity)
            .expect("an attacker queued its attack from this frame's snapshot")
            .book
            .get_unchecked(&ability);
        let ability_name = ability_def.name.clone();
        let ability_spell_school = ability_def.spell_school;
        let mut actual_damage = 0.0;

        // Apply Divine Shield outgoing damage penalty (50%) if attacker has DamageImmunity
        let ds_penalty = if let Some(attacker_auras) = snapshot.active_auras.get(&attacker_entity) {
            if attacker_auras
                .iter()
                .any(|a| a.effect_type == AuraType::DamageImmunity)
            {
                super::constants::DIVINE_SHIELD_DAMAGE_PENALTY
            } else {
                1.0
            }
        } else {
            1.0
        };
        let damage = (damage * ds_penalty).max(0.0);

        if let Ok((_, mut target, target_transform, mut target_auras, _, _)) =
            combatants.get_mut(target_entity)
        {
            if target.is_alive() {
                // Apply damage with absorb shield consideration
                let (dmg, absorbed) = super::combat_core::apply_damage_with_absorb(
                    damage,
                    &mut target,
                    target_auras.as_deref_mut(),
                    ability_spell_school,
                    &ability_name,
                    &mut combat_log,
                );
                actual_damage = dmg;
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
                    actual_damage,
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
            }
        }

        // Update attacker's damage dealt
        if let Ok((_, mut attacker, _, _, _, _)) = combatants.get_mut(attacker_entity) {
            attacker.damage_dealt += actual_damage;
        }
    }

    // Process queued Frost Nova damage
    for aoe in acc.frost_nova_damage {
        let class_ai::QueuedAoeDamage {
            caster: caster_entity,
            target: target_entity,
            damage,
            caster_team,
            caster_slot,
            caster_class,
            target_pos: _target_pos,
            is_crit,
        } = aoe;
        let mut actual_damage = 0.0;

        // Skip if caster died from an instant attack earlier this frame (or entity is gone)
        match combatants.get(caster_entity) {
            Ok((_, caster, _, _, _, _)) if caster.is_alive() => {}
            _ => continue,
        }

        if let Ok((_, mut target, target_transform, mut target_auras, _, _)) =
            combatants.get_mut(target_entity)
        {
            if target.is_alive() {
                // Apply damage with absorb shield consideration (Frost Nova is always Frost school)
                let (dmg, absorbed) = super::combat_core::apply_damage_with_absorb(
                    damage,
                    &mut target,
                    target_auras.as_deref_mut(),
                    super::abilities::SpellSchool::Frost,
                    "Frost Nova",
                    &mut combat_log,
                );
                actual_damage = dmg;
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

                // Log the Frost Nova damage with structured data
                let is_killing_blow = !target.is_alive();
                let is_first_death = is_killing_blow && !target.is_dead;
                if is_first_death {
                    target.is_dead = true;
                }
                let caster_id = combatant_id(caster_team, caster_slot, caster_class);
                let verb = if is_crit { "CRITS" } else { "hits" };
                let message = if absorbed > 0.0 {
                    format!(
                        "{}'s Frost Nova {} {} for {:.0} damage ({:.0} absorbed)",
                        caster_id, verb, target_id, actual_damage, absorbed
                    )
                } else {
                    format!(
                        "{}'s Frost Nova {} {} for {:.0} damage",
                        caster_id, verb, target_id, actual_damage
                    )
                };
                combat_log.log_damage(
                    caster_id.clone(),
                    target_id.clone(),
                    "Frost Nova".to_string(),
                    actual_damage,
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
        if let Ok((_, mut caster, _, _, _, _)) = combatants.get_mut(caster_entity) {
            caster.damage_dealt += actual_damage;
        }
    }
}
/// Kick, Pummel and Wind Shear: each living interrupter decides whom to
/// interrupt ([`class_ai::interrupts::interrupt_turn`] — range, then sight)
/// and the interrupt is queued for `process_interrupts`. Runs after every cast
/// of the frame has started, so a cast begun this frame can be cut.
#[allow(clippy::too_many_arguments)]
pub fn check_interrupts(
    mut commands: Commands,
    mut combat_log: ResMut<CombatLog>,
    mut game_rng: ResMut<GameRng>,
    mut combatants: Query<(Entity, &mut Combatant, &Transform), Without<CastingState>>,
    channeling_targets: Query<&ChannelingState>,
    all_auras: Query<&ActiveAuras>,
    // Every unit mid a cast. Disjoint from `combatants` (which is
    // `Without<CastingState>`), so this read-only borrow of `&Combatant` does
    // not conflict.
    casting_combatants: Query<(Entity, &Combatant, &Transform, &CastingState)>,
    pet_query: Query<&Pet>,
    dr_tracker_query: Query<(Entity, &DRTracker)>,
    // `Option` so a world with no map resolves as an open field.
    map_geometry: Option<Res<super::map_config::ActiveMapGeometry>>,
    ai_profile: Option<Res<super::ai_profile::AiProfiles>>,
    // Each team's situation this frame, as `decide_abilities` left it: carried
    // on every interrupt decision the trace records.
    situations: Option<Res<super::situation::Situations>>,
    celebration: Option<Res<VictoryCelebration>>,
    // Where an interrupter's choice, and a cast it passes over, are traced.
    // `Option` so a world that never inserted it still runs.
    mut decision_trace: Option<ResMut<super::decision_trace::DecisionTrace>>,
) {
    // Don't interrupt during victory celebration
    if celebration.is_some() {
        return;
    }
    // Nobody casting or channelling: nothing to interrupt.
    if casting_combatants.is_empty() && channeling_targets.is_empty() {
        return;
    }

    // Every unit mid a cast or a channel, in entity order.
    let mut casts: std::collections::BTreeMap<Entity, class_ai::interrupts::EnemyCast> =
        std::collections::BTreeMap::new();
    for (e, c, t, cast) in casting_combatants.iter() {
        casts.insert(
            e,
            class_ai::interrupts::EnemyCast::casting(e, c, t.translation, cast),
        );
    }
    for (e, c, t) in combatants.iter() {
        if let Ok(channel) = channeling_targets.get(e) {
            casts.insert(
                e,
                class_ai::interrupts::EnemyCast::channeling(e, c, t.translation, channel),
            );
        }
    }

    let snapshot = {
        let units = combatants
            .iter()
            .map(
                |(entity, combatant, transform)| class_ai::combat_snapshot::SnapshotUnit {
                    entity,
                    combatant,
                    transform,
                    casting: channeling_targets.get(entity).ok().map(|ch| ch.ability),
                    auras: all_auras.get(entity).ok(),
                },
            )
            .chain(
                casting_combatants
                    .iter()
                    .map(|(entity, combatant, transform, cast)| {
                        class_ai::combat_snapshot::SnapshotUnit {
                            entity,
                            combatant,
                            transform,
                            casting: Some(cast.ability),
                            auras: all_auras.get(entity).ok(),
                        }
                    }),
            );
        class_ai::combat_snapshot::CombatSnapshot::from_units(
            units,
            &pet_query,
            &dr_tracker_query,
            map_geometry.as_ref().map_or(&[][..], |m| &m.volumes[..]),
            map_geometry.as_ref().map(|m| m.bounds).unwrap_or_default(),
            ai_profile.map(|p| *p).unwrap_or_default(),
        )
    };

    for (entity, mut combatant, transform) in combatants.iter_mut() {
        let my_pos = transform.translation;
        let ctx = snapshot.context_for(entity).with_situation(
            situations
                .as_deref()
                .map(|s| s.for_team(combatant.team))
                .unwrap_or_default(),
        );
        let Some((interrupt_ability, target_entity, cast_ability)) =
            class_ai::interrupts::interrupt_turn(
                &ctx,
                &combatant,
                my_pos,
                all_auras.get(entity).ok(),
                &casts,
                decision_trace.as_deref_mut(),
            )
        else {
            continue;
        };
        let target_pos = casts.get(&target_entity).map_or(my_pos, |c| c.position);
        class_ai::interrupts::fire_interrupt(
            &mut commands,
            &mut combat_log,
            &mut game_rng,
            &ctx,
            entity,
            &mut combatant,
            my_pos,
            interrupt_ability,
            target_entity,
            target_pos,
            cast_ability,
        );
    }
}
