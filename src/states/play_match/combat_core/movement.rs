//! Movement systems: target following, kiting, fear/polymorph wandering, charging, disengaging.

use super::super::arena_bounds::ArenaBounds;
use super::super::components::*;
use super::super::map_config::ActiveMapGeometry;
use super::super::map_geometry::{
    has_line_of_sight, resolve_movement, rounding, steer_toward_goal, ObstacleVolume, TangentSide,
    EYE_HEIGHT,
};
use super::super::match_config::CharacterClass;
use super::super::movement_config::MovementConfig;
use super::super::situation::Situations;
use super::super::team_plan::{
    hold_position, should_break_cover, should_hold, Stance, TeamPlans, CAMP_ARRIVAL_EPSILON,
    CAMP_ENGAGE_RADIUS,
};
use super::super::{DISENGAGE_SPEED, MELEE_RANGE, WAND_RANGE};
use super::clamp_to_arena;
use bevy::prelude::*;

/// How close a `MovementGoal::Point` directive walks before stopping (units).
/// Prevents oscillation around the exact formation point.
pub const DIRECTIVE_POINT_EPSILON: f32 = 0.25;

/// Slide a proposed step off any obstacle, THEN clamp to arena bounds — the
/// order every mutation site in `move_to_target` uses (obstacle resolution
/// first, arena clamp last). `resolve_movement` never returns a position inside
/// a volume and tangent-slides a blocked step; with no obstacles (BasicArena)
/// it returns `to` unchanged, so this reduces to the prior `clamp_to_arena(to)`
/// and the whole feature is a provable no-op on obstacle-free maps.
fn resolve_and_clamp(
    bounds: &ArenaBounds,
    volumes: &[ObstacleVolume],
    from: Vec3,
    to: Vec3,
) -> Vec3 {
    clamp_to_arena(bounds, resolve_movement(volumes, from, to))
}

/// A mover's effective speed: `base` times every `MovementSpeedSlow` on it and
/// the Druid's Travel Form, in aura order. The one derivation every movement
/// branch uses, so a camper, a directive walk and a pursuit all feel a slow —
/// and a shifted Druid's speed — the same way.
pub fn effective_movement_speed(base: f32, auras: Option<&ActiveAuras>) -> f32 {
    let mut movement_speed = base;
    if let Some(auras) = auras {
        for aura in &auras.auras {
            if matches!(
                aura.effect_type,
                AuraType::MovementSpeedSlow | AuraType::TravelForm
            ) {
                movement_speed *= aura.magnitude;
            }
        }
    }
    movement_speed
}

/// The tangent side a yielding DPS's pursuit holds round an obstacle (AS-386,
/// the team sight yield): which way round, picked for its healer's sight, and
/// until when it holds. Renewed every frame the same obstacle still blocks
/// the pursuit, so it holds for the whole rounding and lapses
/// [`shared.commit_window`](crate::states::play_match::movement_config) after
/// the obstacle stops being in the way.
///
/// It is STORED because a sight preference is not self-reinforcing the way
/// `steer_toward_goal`'s own side scores are (AS-181): the waypoint that keeps
/// the healer in sight moves round the pillar as the mover does, so a side
/// re-picked every frame would flip.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PursuitSide {
    /// The obstacle's index in the map's volumes.
    pub obstacle: usize,
    pub side: TangentSide,
    /// The side lapses after this sim time.
    pub until: f32,
    /// A DPS in danger with no sighted side holds at the pillar's edge, where
    /// its healer still sees it, until this time, then chases on `side`.
    pub hold_until: Option<f32>,
}

/// What a yielding DPS's pursuit keeps in sight: its healer, and whether the
/// DPS is the teammate in danger (which buys the hold at the pillar's edge).
#[derive(Clone, Copy, Debug)]
pub struct SightKeep {
    pub healer: Vec3,
    pub in_danger: bool,
}

/// The pursuit heading of a DPS that yields its healer's sight (AS-386),
/// and the side it holds afterwards. Pure.
///
/// Returns `(heading, held)`. A `None` heading leaves the pursuit to
/// `steer_toward_goal` exactly as before: the path is clear, or both ways
/// round keep the healer in sight (today's self-reinforcing side score
/// decides, and nothing is stored). Otherwise:
///
/// - **A held side** for the same obstacle, not lapsed, is taken again.
/// - **One side keeps sight** (its waypoint, `map_geometry::Rounding::waypoint`, is in
///   the healer's sight at eye height): that side, stored.
/// - **Neither does:** the side whose waypoint is nearer the healer, stored —
///   except that a DPS in danger that its healer still sees holds where it
///   stands for at most `commit` seconds first (the zero heading), then
///   chases on that side.
pub fn sight_kept_heading(
    obstacles: &[ObstacleVolume],
    from: Vec3,
    goal: Vec2,
    keep: SightKeep,
    held: Option<PursuitSide>,
    now: f32,
    commit: f32,
) -> (Option<Vec2>, Option<PursuitSide>) {
    let from_xz = Vec2::new(from.x, from.z);
    let Some(round) = rounding(obstacles, from_xz, goal, from.y) else {
        return (None, held);
    };
    let renewed = |side: TangentSide, hold_until: Option<f32>| PursuitSide {
        obstacle: round.obstacle,
        side,
        until: now + commit,
        hold_until,
    };
    if let Some(h) = held.filter(|h| h.obstacle == round.obstacle && now <= h.until) {
        let heading = match h.hold_until {
            Some(until) if now < until => Vec2::ZERO,
            _ => round.heading(h.side),
        };
        return (Some(heading), Some(renewed(h.side, h.hold_until)));
    }
    let eye = |p: Vec2| Vec3::new(p.x, EYE_HEIGHT, p.y);
    let healer_eye = Vec3::new(keep.healer.x, EYE_HEIGHT, keep.healer.z);
    let sees = |p: Vec2| has_line_of_sight(obstacles, eye(p), healer_eye);
    let side = match (sees(round.left_waypoint), sees(round.right_waypoint)) {
        (true, false) => TangentSide::Left,
        (false, true) => TangentSide::Right,
        (true, true) => return (None, held),
        (false, false) => {
            let healer = Vec2::new(keep.healer.x, keep.healer.z);
            let side =
                if round.left_waypoint.distance(healer) <= round.right_waypoint.distance(healer) {
                    TangentSide::Left
                } else {
                    TangentSide::Right
                };
            if keep.in_danger && sees(from_xz) {
                return (Some(Vec2::ZERO), Some(renewed(side, Some(now + commit))));
            }
            side
        }
    };
    (Some(round.heading(side)), Some(renewed(side, None)))
}

/// How long a [`PursuitSide`] holds past the last frame its obstacle blocked
/// the pursuit, when no `movement.ron` is loaded: `shared.commit_window`'s
/// shipped value.
const PURSUIT_SIDE_COMMIT: f32 = 0.6;

/// What `entity`'s pursuit must keep in sight: its healer, while its team's
/// `sight_yield` says it yields to that healer. `None` with no
/// classification, and always under `Legacy`, which classifies no yield.
fn sight_keep(
    situations: Option<&Situations>,
    positions: &std::collections::BTreeMap<Entity, (Vec3, u8)>,
    entity: Entity,
    team: u8,
) -> Option<SightKeep> {
    let situation = situations?.teams.get(team.checked_sub(1)? as usize)?;
    let pair = situation.sight_yield_of(entity)?;
    let &(healer, _) = positions.get(&pair.healer)?;
    Some(SightKeep {
        healer,
        in_danger: situation
            .ally_in_danger
            .is_some_and(|w| w.subject == entity),
    })
}

/// Whether normal pursuit keeps closing on its target this frame.
///
/// Beyond `stop_distance` (the class's preferred range) it always does. Inside
/// it, a unit that `walks_for_sight` keeps closing while `in_sight` says a
/// pillar stands between it and its target: stopping there parks it where
/// every cast it has is refused `LosBlocked`, and against a target that is
/// itself parked — a lone healer out of mana behind a Nagrand pillar — nothing
/// else moves either, and the match draws at the cap (AS-360). The walk is the
/// pursuit's own tangent-steered step, so it rounds the pillar and stops the
/// frame sight returns.
///
/// Healers do not walk for sight (`walks_for_sight` false): their posture
/// machine owns where they stand, and a healer out of sight of the enemy it
/// targets is usually there on purpose — in cover. The Mage and Hunter answer
/// occlusion with their own ENGAGE machine (`los_seek` and the occlusion
/// chase); the pursuit sees them in range and out of sight only on the odd
/// frames before that machine has, and walks them there too.
///
/// Both AI profiles: the pursuit belongs to neither, and a unit parked with
/// every cast refused is a defect under either.
///
/// `in_sight` is evaluated only when it can change the answer.
pub fn pursuit_closes(
    distance: f32,
    stop_distance: f32,
    walks_for_sight: bool,
    in_sight: impl FnOnce() -> bool,
) -> bool {
    distance > stop_distance || (walks_for_sight && !in_sight())
}

pub fn move_to_target(
    countdown: Res<MatchCountdown>,
    time: Res<Time>,
    mut commands: Commands,
    mut combatants: Query<(
        Entity,
        &mut Transform,
        &Combatant,
        Option<&ActiveAuras>,
        Option<&CastingState>,
        Option<&ChargingState>,
        Option<&ChannelingState>,
        Option<&DisengagingState>,
        Option<&MovementDirective>,
    )>,
    orbs: Query<&Transform, (With<ShadowSightOrb>, Without<Combatant>)>,
    pet_query: Query<&Pet>,
    // OOM wand-fallback latch (Mage): when set, the Mage's ENGAGE pursuit stops
    // at wand range instead of its Frostbolt-safe preferred range so the wand
    // auto-attack fires. Read-only; owned/updated by `evaluate_dps_posture`.
    kite_query: Query<&KitePosture>,
    map_geometry: Res<ActiveMapGeometry>,
    // Team-level plan. `Option` so a scene that never inserted it simply behaves
    // as it always has, rather than panicking.
    team_plans: Option<Res<TeamPlans>>,
    // The team sight yield (AS-386): a yielding DPS's pursuit takes, and
    // holds, the tangent side that keeps its healer in sight. `Option` as
    // `team_plans` is: no classification, no yield.
    situations: Option<Res<Situations>>,
    movement_config: Option<Res<MovementConfig>>,
    pursuit_sides: Query<&PursuitSide>,
) {
    // Don't allow movement until gates open
    if !countdown.gates_opened {
        return;
    }

    let dt = time.delta_secs();
    // Absolute sim-time, compared against MovementDirective.expires deadlines.
    let now = time.elapsed_secs();

    // Build a snapshot of all combatant positions and team info for lookups.
    // BTreeMap (not HashMap) so iteration order is deterministic by Entity —
    // downstream consumers iterate this map (kiting's nearest-enemy scan, and
    // the position scorer's threat inputs), and Rust's randomized HashMap
    // hasher would otherwise make tie-breaks between equidistant enemies vary
    // across runs, breaking byte-identical determinism at a fixed seed. See
    // docs/solutions/implementation-patterns/ai-decision-trace.md (determinism
    // section) and the matching comments in auto_attack.rs.
    let positions: std::collections::BTreeMap<Entity, (Vec3, u8)> = combatants
        .iter()
        .map(|(entity, transform, combatant, _, _, _, _, _, _)| {
            (entity, (transform.translation, combatant.team))
        })
        .collect();

    // Position + team + HP fraction + pet flag, for the camp's line-of-sight
    // cycle and its threat scan. Separate from `positions` so that map's shape
    // (and every other consumer of it) is untouched.
    //
    // LIVING units only. `positions` deliberately carries corpses too (its other
    // consumers are keyed lookups that never scan it), and the camp is the one
    // place that DOES scan: without this filter a dead enemy still counted as a
    // threat to hide from, and could even be the "nearest enemy" the camp uses
    // to decide whether to release.
    let living: std::collections::BTreeMap<Entity, (Vec3, u8, f32, bool)> = combatants
        .iter()
        .filter(|(_, _, c, _, _, _, _, _, _)| c.is_alive())
        .map(|(entity, transform, c, _, _, _, _, _, _)| {
            let frac = if c.max_health > 0.0 {
                c.current_health / c.max_health
            } else {
                0.0
            };
            let is_pet = pet_query.get(entity).is_ok();
            (entity, (transform.translation, c.team, frac, is_pet))
        })
        .collect();

    // Move each combatant towards their target if needed
    for (
        entity,
        mut transform,
        combatant,
        auras,
        casting_state,
        charging_state,
        channeling_state,
        disengaging_state,
        movement_directive,
    ) in combatants.iter_mut()
    {
        // MOVEMENT DIRECTIVE EXPIRY — checked before EVERY early-continue
        // below (death, casting, channeling, root/stun). `expires` is an
        // absolute sim-time deadline: a directive issued pre-stun must be
        // gone on the first post-stun frame, not survive the stun frozen at
        // full lifetime and move the healer along a stale vector. Expired
        // directives are removed and never executed.
        let movement_directive = match movement_directive {
            Some(d) if now >= d.expires => {
                commands.entity(entity).remove::<MovementDirective>();
                None
            }
            other => other,
        };

        if !combatant.is_alive() {
            continue;
        }

        // Cannot move while casting (WoW mechanic)
        if casting_state.is_some() {
            continue;
        }

        // Cannot move while channeling (WoW mechanic)
        if channeling_state.is_some() {
            continue;
        }

        // Check for movement-preventing CC and wandering CC
        let (is_rooted_or_stunned, fear_direction, polymorph_direction) = if let Some(auras) = auras
        {
            let rooted_or_stunned = auras.auras.iter().any(|a| {
                matches!(
                    a.effect_type,
                    AuraType::Root | AuraType::Stun | AuraType::Incapacitate | AuraType::Cyclone
                )
            });
            let fear_dir = auras
                .auras
                .iter()
                .find(|a| a.effect_type == AuraType::Fear)
                .map(|a| a.fear_direction);
            let poly_dir = auras
                .auras
                .iter()
                .find(|a| a.effect_type == AuraType::Polymorph)
                .map(|a| a.fear_direction); // Polymorph reuses fear_direction for wandering
            (rooted_or_stunned, fear_dir, poly_dir)
        } else {
            (false, None, None)
        };

        // Cannot move at all if rooted or stunned
        if is_rooted_or_stunned {
            continue;
        }

        let my_pos = transform.translation;

        // FEARED BEHAVIOR: If feared, run in random direction (ignoring normal movement)
        if let Some((dir_x, dir_z)) = fear_direction {
            let direction = Vec3::new(dir_x, 0.0, dir_z).normalize_or_zero();

            if direction != Vec3::ZERO {
                // Feared targets run at normal movement speed (no slows applied during fear)
                let move_distance = combatant.base_movement_speed * dt;

                // Move in fear direction (slide off obstacles, then clamp to arena)
                let from = transform.translation;
                let proposed = from + direction * move_distance;
                transform.translation =
                    resolve_and_clamp(&map_geometry.bounds, &map_geometry.volumes, from, proposed);

                // Rotate to face direction of travel
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }

            continue; // Skip normal movement logic while feared
        }

        // POLYMORPH BEHAVIOR: If polymorphed, wander slowly (like Fear but at 50% speed)
        if let Some((dir_x, dir_z)) = polymorph_direction {
            let direction = Vec3::new(dir_x, 0.0, dir_z).normalize_or_zero();

            if direction != Vec3::ZERO {
                // Polymorphed targets wander at 20% of normal movement speed (sheep waddle slowly!)
                let move_distance = combatant.base_movement_speed * 0.2 * dt;

                // Move in polymorph direction (slide off obstacles, then clamp to arena)
                let from = transform.translation;
                let proposed = from + direction * move_distance;
                transform.translation =
                    resolve_and_clamp(&map_geometry.bounds, &map_geometry.volumes, from, proposed);

                // Rotate to face direction of travel
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }

            continue; // Skip normal movement logic while polymorphed
        }

        // CHARGING BEHAVIOR: If charging, move at high speed toward target ignoring slows
        if let Some(charge_state) = charging_state {
            let Some(&(target_pos, _)) = positions.get(&charge_state.target) else {
                // Target doesn't exist, cancel charge
                commands.entity(entity).remove::<ChargingState>();
                continue;
            };

            let distance = my_pos.distance(target_pos);

            // If we've reached melee range, end the charge
            if distance <= MELEE_RANGE {
                commands.entity(entity).remove::<ChargingState>();

                info!(
                    "Team {} {} completes charge!",
                    combatant.team,
                    combatant.class.name()
                );

                continue; // Will use normal movement/combat next frame
            }

            // Calculate direction to target
            let direction = Vec3::new(target_pos.x - my_pos.x, 0.0, target_pos.z - my_pos.z)
                .normalize_or_zero();

            if direction != Vec3::ZERO {
                // Charge speed: 4x normal movement speed, ignores slows
                const CHARGE_SPEED_MULTIPLIER: f32 = 4.0;
                let charge_speed = combatant.base_movement_speed * CHARGE_SPEED_MULTIPLIER;
                let move_distance = charge_speed * dt;

                // Move towards target (slide off obstacles, then clamp to arena).
                // The mid-dash resolve is the safety net; the trigger-time path
                // check in warrior AI rejects Charge across an obstacle up front.
                let from = transform.translation;
                let proposed = from + direction * move_distance;
                transform.translation =
                    resolve_and_clamp(&map_geometry.bounds, &map_geometry.volumes, from, proposed);

                // Rotate to face target
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }

            continue; // Skip normal movement logic while charging
        }

        // DISENGAGE BEHAVIOR: If disengaging, leap backward at high speed
        if let Some(disengage) = disengaging_state {
            if disengage.distance_remaining > 0.0 {
                let move_amount = DISENGAGE_SPEED * dt;
                let from = transform.translation;
                let new_pos = from + disengage.direction * move_amount;

                // Slide off obstacles, then clamp to arena bounds
                transform.translation =
                    resolve_and_clamp(&map_geometry.bounds, &map_geometry.volumes, from, new_pos);

                // Decrement distance remaining
                let remaining = disengage.distance_remaining - move_amount;
                if remaining <= 0.0 {
                    commands.entity(entity).remove::<DisengagingState>();
                } else {
                    commands.entity(entity).try_insert(DisengagingState {
                        direction: disengage.direction,
                        distance_remaining: remaining,
                    });
                }

                continue; // Skip normal movement logic while disengaging
            } else {
                commands.entity(entity).remove::<DisengagingState>();
            }
        }

        // PILLAR CAMP (team strategy): hold cover and make the enemy cross the
        // arena. Sits ABOVE the posture directive deliberately — the strategy
        // layer subordinates execution, it does not race it.
        //
        // This block used to live down in the pursuit path, below the directive
        // branch's `continue`. Melee reach that path so their camp worked; healers
        // almost always carry a posture directive, so their camp only fired in the
        // GAPS between directives. The result was a Priest oscillating 17-26yd
        // from its pillar for a whole match — never arriving, never healing,
        // because two layers were issuing contradictory orders with no arbitration.
        //
        // Still below CC and the scripted Charge/Disengage bursts: a camp must not
        // override being stunned, and a committed dash outranks a positioning
        // intent.
        //
        // Threat reference is the NEAREST enemy, not the kill target: you take
        // cover from whoever is coming at you, which for a healer is rarely the
        // unit it is trying to damage.
        let camp_spot = team_plans.as_ref().and_then(|plans| {
            let plan = plans.for_team(combatant.team);
            if plan.stance != Stance::Hold {
                return None;
            }
            // A pet is not a member of the team's plan — it owes its owner a
            // flank, not the pillar. Without this a Felhunter/Boar on a camping
            // comp abandoned its owner and walked to the hold spot, because pets
            // share their owner's `team` and so matched the stance check.
            if pet_query.get(entity).is_ok() {
                return None;
            }
            let anchor = plan.anchor?;
            let nearest = living
                .iter()
                .filter(|(_, (_, team, _, _))| *team != combatant.team)
                .map(|(_, (pos, _, _, _))| (my_pos.distance(*pos), *pos))
                .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let (nearest_d, nearest_pos) = nearest?;
            if !should_hold(Some(nearest_d), CAMP_ENGAGE_RADIUS) {
                return None;
            }
            // LINE-OF-SIGHT CYCLE. A healer alternates between a POKE spot (sees
            // the ally, can heal) and a DUCK spot (maximally occluded). Holding a
            // sight-line permanently is what left the Priest exposed to the enemy
            // Warlock 70% of the match and mana-burned to nothing.
            //
            // The cycle needs no state machine: casting units are planted and
            // `continue` above this branch, so all that is required per tick is
            // "does anyone need healing?" — if yes, break cover for the cast; if
            // no, hide.
            let keep_sighted = combatant
                .class
                .is_healer()
                .then(|| {
                    // Most-injured living HEALABLE ally, and how hurt it is. Pets are
                    // excluded because no healer in this codebase ever targets one
                    // (every heal/shield/medic-chase predicate filters `is_pet`), so
                    // including them made the healer break cover to hold sight of a
                    // Felhunter or Hunter pet it could not heal — and, because pets
                    // are squishy and engage first, that pet was usually the
                    // lowest-HP unit on the team, hiding the teammate that did need
                    // the line.
                    let worst = living
                        .iter()
                        .filter(|(e, (_, team, _, is_pet))| {
                            *team == combatant.team && **e != entity && !*is_pet
                        })
                        .map(|(_, (pos, _, hp, _))| (*hp, *pos))
                        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                    let (hp, pos) = worst?;
                    should_break_cover(Some(hp)).then_some(Vec2::new(pos.x, pos.z))
                })
                .flatten();

            // Every living enemy, so the spot maximises how many of them lose
            // sight — hiding from only the nearest left the other caster free.
            let threats: Vec<Vec2> = living
                .iter()
                .filter(|(_, (_, team, _, _))| *team != combatant.team)
                .map(|(_, (pos, _, _, _))| Vec2::new(pos.x, pos.z))
                .collect();
            hold_position(&map_geometry.volumes, anchor, &threats, keep_sighted)
                .map(|spot| (spot, nearest_pos))
        });

        if let Some((spot, face_pos)) = camp_spot {
            let dest = Vec3::new(spot.x, my_pos.y, spot.y);
            let to_spot = dest - my_pos;
            if Vec2::new(to_spot.x, to_spot.z).length() > CAMP_ARRIVAL_EPSILON {
                let dir = steer_toward_goal(
                    &map_geometry.volumes,
                    Vec2::new(my_pos.x, my_pos.z),
                    spot,
                    my_pos.y,
                )
                .unwrap_or_else(|| Vec2::new(to_spot.x, to_spot.z).normalize_or_zero());
                // Same speed derivation as every other branch: base speed times
                // MovementSpeedSlow, or a camper would ignore Frost Trap.
                let movement_speed = effective_movement_speed(combatant.base_movement_speed, auras);
                let step = Vec3::new(dir.x, 0.0, dir.y) * movement_speed * dt;
                transform.translation = resolve_and_clamp(
                    &map_geometry.bounds,
                    &map_geometry.volumes,
                    my_pos,
                    my_pos + step,
                );
            }
            // Face the approach while holding, so the camp reads as deliberate.
            let facing = face_pos - transform.translation;
            if facing.length_squared() > 1e-6 {
                transform.rotation = Quat::from_rotation_y(facing.x.atan2(facing.z));
            }
            // A camp overrides the posture directive; drop it so the unit does not
            // resume a stale walk the instant the camp releases.
            commands.entity(entity).remove::<MovementDirective>();
            continue;
        }

        // MOVEMENT DIRECTIVE: execute an unexpired directive issued by class
        // AI (healer postures + the Mage/Hunter ENGAGE/KITE machine). Ladder
        // position is deliberate: casting/channeling/root/stun continue ABOVE
        // (R12 — posture movement happens in cast gaps and never overrides CC),
        // and Charge/Disengage are scripted bursts that outrank it. This is now
        // the ONLY kiting path — the legacy `kiting_timer` branch is gone.
        // Entities without the component fall through to normal pursuit.
        if let Some(directive) = movement_directive {
            // Effective movement speed: base × MovementSpeedSlow multipliers.
            let movement_speed = effective_movement_speed(combatant.base_movement_speed, auras);
            let mut move_distance = movement_speed * dt;

            let direction = match directive.goal {
                MovementGoal::Direction(dir) => Vec3::new(dir.x, 0.0, dir.y).normalize_or_zero(),
                MovementGoal::Point(point) => {
                    let to_point = Vec3::new(point.x - my_pos.x, 0.0, point.z - my_pos.z);
                    let distance = to_point.length();
                    if distance <= DIRECTIVE_POINT_EPSILON {
                        // Arrived: hold position until the directive expires
                        // or the issuing AI replaces it.
                        Vec3::ZERO
                    } else {
                        // Don't overshoot the point this frame.
                        move_distance = move_distance.min(distance);
                        // Tangent-steer around any obstacle between here and the
                        // point (chase/medic/formation walks); direct otherwise.
                        steer_toward_goal(
                            &map_geometry.volumes,
                            Vec2::new(my_pos.x, my_pos.z),
                            Vec2::new(point.x, point.z),
                            my_pos.y,
                        )
                        .map(|s| Vec3::new(s.x, 0.0, s.y))
                        .unwrap_or_else(|| to_point.normalize_or_zero())
                    }
                }
                MovementGoal::Entity(target) => {
                    if let Some(&(target_pos, _)) = positions.get(&target) {
                        // Tangent-steer around any obstacle between here and the
                        // pursued entity (DIP chase); direct otherwise.
                        steer_toward_goal(
                            &map_geometry.volumes,
                            Vec2::new(my_pos.x, my_pos.z),
                            Vec2::new(target_pos.x, target_pos.z),
                            my_pos.y,
                        )
                        .map(|s| Vec3::new(s.x, 0.0, s.y))
                        .unwrap_or_else(|| {
                            Vec3::new(target_pos.x - my_pos.x, 0.0, target_pos.z - my_pos.z)
                                .normalize_or_zero()
                        })
                    } else {
                        // Target despawned — hold; the directive's TTL or the
                        // issuing AI's abort logic cleans it up.
                        Vec3::ZERO
                    }
                }
            };

            if direction != Vec3::ZERO {
                // Slide off obstacles, then clamp to arena bounds
                let from = transform.translation;
                let proposed = from + direction * move_distance;
                transform.translation =
                    resolve_and_clamp(&map_geometry.bounds, &map_geometry.volumes, from, proposed);

                // Rotate to face direction of travel
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }

            continue; // Directive owns this frame's movement
        }

        // NORMAL MOVEMENT: Get target position
        let Some(target_entity) = combatant.target else {
            // Pets with no target follow their owner
            if let Ok(pet) = pet_query.get(entity) {
                if let Some(&(owner_pos, _)) = positions.get(&pet.owner) {
                    let dist_to_owner = my_pos.distance(owner_pos);
                    if dist_to_owner > 3.0 {
                        // Tangent-steer around any obstacle between the pet and
                        // its owner; direct otherwise.
                        let direction = steer_toward_goal(
                            &map_geometry.volumes,
                            Vec2::new(my_pos.x, my_pos.z),
                            Vec2::new(owner_pos.x, owner_pos.z),
                            my_pos.y,
                        )
                        .map(|s| Vec3::new(s.x, 0.0, s.y))
                        .unwrap_or_else(|| {
                            Vec3::new(owner_pos.x - my_pos.x, 0.0, owner_pos.z - my_pos.z)
                                .normalize_or_zero()
                        });
                        if direction != Vec3::ZERO {
                            let movement_speed =
                                effective_movement_speed(combatant.base_movement_speed, auras);
                            let move_distance = movement_speed * dt;
                            // Slide off obstacles, then clamp to arena bounds
                            let from = transform.translation;
                            let proposed = from + direction * move_distance;
                            transform.translation = resolve_and_clamp(
                                &map_geometry.bounds,
                                &map_geometry.volumes,
                                from,
                                proposed,
                            );
                            let target_rotation =
                                Quat::from_rotation_y(direction.x.atan2(direction.z));
                            transform.rotation = target_rotation;
                        }
                    }
                }
                continue;
            }

            // No target available (likely facing all-stealth team)
            // When orbs are spawned, ALL combatants (even stealthed) should seek them
            // to break the stalemate. This represents accepting the reveal to gain vision.

            // Find nearest Shadow Sight orb (if any exist)
            let nearest_orb_pos = orbs
                .iter()
                .map(|orb_transform| orb_transform.translation)
                .min_by(|a, b| {
                    let dist_a = my_pos.distance(*a);
                    let dist_b = my_pos.distance(*b);
                    dist_a.partial_cmp(&dist_b).unwrap()
                });

            // Determine destination: nearest orb if available, otherwise center
            let destination = nearest_orb_pos.unwrap_or(Vec3::ZERO);
            let distance_to_destination = my_pos.distance(destination);

            // Only move if we're far from destination (> 2.5 units for orbs, > 5 units for center)
            let stop_distance = if nearest_orb_pos.is_some() { 2.5 } else { 5.0 };
            if distance_to_destination > stop_distance {
                let direction = Vec3::new(destination.x - my_pos.x, 0.0, destination.z - my_pos.z)
                    .normalize_or_zero();

                if direction != Vec3::ZERO {
                    // Calculate effective movement speed
                    let movement_speed =
                        effective_movement_speed(combatant.base_movement_speed, auras);

                    // Move towards destination (slide off obstacles, then clamp to arena)
                    let move_distance = movement_speed * dt;
                    let from = transform.translation;
                    let proposed = from + direction * move_distance;
                    transform.translation = resolve_and_clamp(
                        &map_geometry.bounds,
                        &map_geometry.volumes,
                        from,
                        proposed,
                    );

                    // Rotate to face destination
                    let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                    transform.rotation = target_rotation;
                }
            }

            continue;
        };

        let Some(&(target_pos, _)) = positions.get(&target_entity) else {
            continue;
        };

        let distance = my_pos.distance(target_pos);

        // Use class-specific preferred range, or pet-type preferred range for pets
        let stop_distance = if let Ok(pet) = pet_query.get(entity) {
            pet.pet_type.preferred_range()
        } else if combatant.class == CharacterClass::Mage
            && kite_query.get(entity).map(|k| k.wand_oom).unwrap_or(false)
        {
            // Out-of-mana Mage wand fallback: close to wand range (30) instead
            // of the Frostbolt-safe preferred range (38, outside wand range) so
            // the equipped wand auto-attack fires and chips the kill target
            // through the mana refractory. The latch has hysteresis (see
            // `update_oom_wand_latch`), so this standoff does not strobe 38<->30
            // as mana sawtooths across one Frostbolt's worth. Inert at healthy
            // mana (latch false) — pursuit is byte-identical there.
            WAND_RANGE
        } else if combatant.druid_duel_melee {
            // A dueling Druid that cannot pay for a Moonfire closes to swing
            // its staff (`class_ai::druid::duel_melee`). Never latched outside
            // a duel, so team pursuit is unchanged.
            crate::states::play_match::class_ai::druid::DRUID_DUEL_MELEE_STOP
        } else {
            combatant.class.preferred_range()
        };

        // Move towards the target while out of range — or, for a unit that does
        // not hold ground in cover, while in range but out of its sight. The
        // sight test is the casts' own (`has_line_of_sight` between the two
        // positions), so the walk ends exactly where a cast at the target
        // stops being refused `LosBlocked`. Always sighted on an obstacle-free
        // map, so BasicArena pursuit is unchanged.
        let walks_for_sight = !combatant.class.is_healer();
        if pursuit_closes(distance, stop_distance, walks_for_sight, || {
            has_line_of_sight(&map_geometry.volumes, my_pos, target_pos)
        }) {
            // Calculate direction to target (only in XZ plane, keep Y constant).
            // Tangent-steer around any obstacle between here and the target so a
            // pursuer rounds a pillar in a clean arc instead of oozing along its
            // surface; direct pursuit otherwise (and on obstacle-free maps).
            // A DPS that yields its healer's sight (AS-386) takes the side
            // that keeps it, and holds that side ([`sight_kept_heading`]).
            let goal = Vec2::new(target_pos.x, target_pos.z);
            let sight_kept = sight_keep(situations.as_deref(), &positions, entity, combatant.team)
                .and_then(|keep| {
                    let commit = movement_config
                        .as_deref()
                        .map_or(PURSUIT_SIDE_COMMIT, |c| c.shared.commit_window);
                    let held = pursuit_sides.get(entity).ok().copied();
                    let (heading, side) = sight_kept_heading(
                        &map_geometry.volumes,
                        my_pos,
                        goal,
                        keep,
                        held,
                        now,
                        commit,
                    );
                    if let Some(side) = side.filter(|s| Some(*s) != held) {
                        commands.entity(entity).try_insert(side);
                    }
                    heading
                });
            let direction = sight_kept
                .or_else(|| {
                    steer_toward_goal(
                        &map_geometry.volumes,
                        Vec2::new(my_pos.x, my_pos.z),
                        goal,
                        my_pos.y,
                    )
                })
                .map(|s| Vec3::new(s.x, 0.0, s.y))
                .unwrap_or_else(|| {
                    Vec3::new(
                        target_pos.x - my_pos.x,
                        0.0, // Don't move vertically
                        target_pos.z - my_pos.z,
                    )
                    .normalize_or_zero()
                });

            if direction != Vec3::ZERO {
                // Calculate effective movement speed (base * aura modifiers)
                let movement_speed = effective_movement_speed(combatant.base_movement_speed, auras);

                // Move towards target (slide off obstacles, then clamp to arena)
                let move_distance = movement_speed * dt;
                let from = transform.translation;
                let proposed = from + direction * move_distance;
                transform.translation =
                    resolve_and_clamp(&map_geometry.bounds, &map_geometry.volumes, from, proposed);

                // Rotate to face target
                let target_rotation = Quat::from_rotation_y(direction.x.atan2(direction.z));
                transform.rotation = target_rotation;
            }
        }
    }
}

/// The team sight yield's pursuit side (AS-386, [`sight_kept_heading`]), on
/// one cylinder at the origin, radius 3. A mover 10yd south of it chases a
/// goal 10yd north: the pillar blocks the path, the two ways round are equal,
/// and `steer_toward_goal`'s default is the left (west) side.
#[cfg(test)]
mod sight_side_tests {
    use super::*;

    const NOW: f32 = 10.0;
    const COMMIT: f32 = 0.6;

    fn pillar() -> Vec<ObstacleVolume> {
        vec![ObstacleVolume::Cylinder {
            center_xz: Vec2::ZERO,
            radius: 3.0,
            base_y: 0.0,
            height: 5.0,
        }]
    }

    const FROM: Vec3 = Vec3::new(0.0, 1.0, -10.0);
    const GOAL: Vec2 = Vec2::new(0.0, 10.0);

    fn keep(x: f32, z: f32) -> SightKeep {
        SightKeep {
            healer: Vec3::new(x, 1.0, z),
            in_danger: false,
        }
    }

    fn round() -> crate::states::play_match::map_geometry::Rounding {
        rounding(&pillar(), Vec2::new(FROM.x, FROM.z), GOAL, FROM.y).expect("blocked")
    }

    /// The pair: a healer east sees only the right (east) way round, and the
    /// pursuit takes it — against the default, west — and stores it; a
    /// healer west sees only the left, and gets the left.
    #[test]
    fn a_yielding_pursuit_takes_the_side_its_healer_sees() {
        let r = round();
        assert_eq!(r.default, TangentSide::Left, "the stage's default side");
        let (heading, held) =
            sight_kept_heading(&pillar(), FROM, GOAL, keep(12.0, 0.0), None, NOW, COMMIT);
        assert_eq!(heading, Some(r.right));
        assert_eq!(held.map(|h| h.side), Some(TangentSide::Right));
        let (heading, held) =
            sight_kept_heading(&pillar(), FROM, GOAL, keep(-12.0, 0.0), None, NOW, COMMIT);
        assert_eq!(heading, Some(r.left));
        assert_eq!(held.map(|h| h.side), Some(TangentSide::Left));
    }

    /// Both ways round keep a healer behind the mover in sight: today's side
    /// score decides (no heading of its own), and nothing is stored.
    #[test]
    fn both_sides_sighted_leaves_the_default() {
        let (heading, held) =
            sight_kept_heading(&pillar(), FROM, GOAL, keep(0.0, -25.0), None, NOW, COMMIT);
        assert_eq!(heading, None);
        assert_eq!(held, None);
    }

    /// The held side: picked east for an east healer, it is taken again a
    /// frame later when the healer stands west, where re-picking would flip
    /// it. The control: the same side lapsed (past its `until`) is re-picked
    /// for the west healer.
    #[test]
    fn the_side_holds_once_picked() {
        let r = round();
        let (_, held) =
            sight_kept_heading(&pillar(), FROM, GOAL, keep(12.0, 0.0), None, NOW, COMMIT);
        let held = held.expect("stored");
        let next = NOW + 1.0 / 60.0;
        let (heading, renewed) = sight_kept_heading(
            &pillar(),
            FROM,
            GOAL,
            keep(-12.0, 0.0),
            Some(held),
            next,
            COMMIT,
        );
        assert_eq!(heading, Some(r.right), "the held side flipped");
        assert_eq!(renewed.map(|h| h.until), Some(next + COMMIT), "not renewed");
        let lapsed = held.until + 0.01;
        let (heading, _) = sight_kept_heading(
            &pillar(),
            FROM,
            GOAL,
            keep(-12.0, 0.0),
            Some(held),
            lapsed,
            COMMIT,
        );
        assert_eq!(heading, Some(r.left), "a lapsed side still held");
    }

    /// Neither way round is in a healer's sight (it stands just behind the
    /// pillar's far side): the side whose waypoint is nearer it, stored. The
    /// control mirrors the healer, and the side with it.
    #[test]
    fn neither_side_sighted_takes_the_side_nearer_the_healer() {
        let r = round();
        let (heading, held) =
            sight_kept_heading(&pillar(), FROM, GOAL, keep(0.3, 5.0), None, NOW, COMMIT);
        assert_eq!(heading, Some(r.right));
        assert_eq!(
            held.map(|h| (h.side, h.hold_until)),
            Some((TangentSide::Right, None))
        );
        let (heading, _) =
            sight_kept_heading(&pillar(), FROM, GOAL, keep(-0.3, 5.0), None, NOW, COMMIT);
        assert_eq!(heading, Some(r.left));
    }

    /// A DPS in danger that its healer still sees, with neither way round in
    /// that healer's sight, holds where it stands for the commit window, then
    /// chases on the side it stored. Its healer stands east, behind two posts
    /// that hide both peel-off waypoints of a mover hugging the pillar's east
    /// skin, but not the mover. The control: not in danger, it goes at once.
    #[test]
    fn a_dps_in_danger_holds_at_the_edge_then_chases() {
        let post = |z: f32| ObstacleVolume::Cylinder {
            center_xz: Vec2::new(8.0, z),
            radius: 0.5,
            base_y: 0.0,
            height: 5.0,
        };
        let obstacles = vec![
            ObstacleVolume::Cylinder {
                center_xz: Vec2::ZERO,
                radius: 3.0,
                base_y: 0.0,
                height: 5.0,
            },
            post(1.3),
            post(-1.3),
        ];
        let from = Vec3::new(3.4, 1.0, 0.0);
        let goal = Vec2::new(-10.0, 0.0);
        let healer = SightKeep {
            healer: Vec3::new(12.0, 1.0, 0.0),
            in_danger: true,
        };
        let (heading, held) = sight_kept_heading(&obstacles, from, goal, healer, None, NOW, COMMIT);
        assert_eq!(heading, Some(Vec2::ZERO), "did not hold");
        let held = held.expect("stored");
        assert_eq!(held.hold_until, Some(NOW + COMMIT));
        let r = rounding(&obstacles, Vec2::new(from.x, from.z), goal, from.y).unwrap();
        // Still holding a frame before the window ends (the side renewed)...
        let late = NOW + COMMIT - 0.05;
        let (heading, held) =
            sight_kept_heading(&obstacles, from, goal, healer, Some(held), late, COMMIT);
        assert_eq!(heading, Some(Vec2::ZERO), "the hold ended early");
        let held = held.expect("renewed");
        // ...and chasing on the stored side once it has.
        let after = NOW + COMMIT + 0.01;
        let (heading, _) =
            sight_kept_heading(&obstacles, from, goal, healer, Some(held), after, COMMIT);
        assert_eq!(
            heading,
            Some(r.heading(held.side)),
            "did not chase after the hold"
        );
        let calm = SightKeep {
            in_danger: false,
            ..healer
        };
        let (heading, held) = sight_kept_heading(&obstacles, from, goal, calm, None, NOW, COMMIT);
        assert_eq!(heading.map(|h| h != Vec2::ZERO), Some(true));
        assert_eq!(held.and_then(|h| h.hold_until), None);
    }

    /// An open path is not the pursuit side's: no heading, the held side
    /// passed through untouched.
    #[test]
    fn a_clear_path_leaves_the_pursuit_alone() {
        let goal = Vec2::new(10.0, -10.0);
        let held = Some(PursuitSide {
            obstacle: 0,
            side: TangentSide::Right,
            until: NOW,
            hold_until: None,
        });
        assert_eq!(
            sight_kept_heading(&pillar(), FROM, goal, keep(12.0, 0.0), held, NOW, COMMIT),
            (None, held)
        );
    }
}

#[cfg(test)]
mod pursuit_sight_tests {
    use super::pursuit_closes;

    /// Beyond its range a pursuer closes, sighted or not, healer or not.
    #[test]
    fn beyond_range_always_closes() {
        for walks in [true, false] {
            for sight in [true, false] {
                assert!(pursuit_closes(30.0, 28.0, walks, || sight));
            }
        }
    }

    /// In range and in sight it stands, healer or not.
    #[test]
    fn in_range_and_in_sight_stands() {
        assert!(!pursuit_closes(20.0, 28.0, true, || true));
        assert!(!pursuit_closes(20.0, 28.0, false, || true));
    }

    /// The pair: in range, out of sight. The unit that walks for sight keeps
    /// closing; the one that holds ground in cover (a healer) stands.
    #[test]
    fn in_range_out_of_sight_closes_only_when_walking_for_sight() {
        assert!(pursuit_closes(20.0, 28.0, true, || false));
        assert!(!pursuit_closes(20.0, 28.0, false, || false));
    }

    /// Exactly at its range the pursuer has arrived: the strict `>` the
    /// pursuit always used.
    #[test]
    fn at_range_has_arrived() {
        assert!(!pursuit_closes(28.0, 28.0, true, || true));
    }

    /// Sight is not asked when it cannot change the answer.
    #[test]
    fn sight_is_asked_only_when_it_decides() {
        let ask = || -> bool { panic!("sight asked") };
        assert!(pursuit_closes(30.0, 28.0, true, ask));
        assert!(!pursuit_closes(20.0, 28.0, false, ask));
    }
}
