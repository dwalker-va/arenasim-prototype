//! Hunter Freezing Trap "dip" — a committed walk to set a Freezing Trap on the
//! enemy healer, mirroring the Paladin HoJ dip and Priest Scream dip.
//!
//! ## Why
//! A placed trap triggers on the FIRST enemy inside its radius (`traps.rs`), so
//! a Freezing Trap thrown at the Hunter↔healer midpoint while a melee chases is
//! eaten by the chaser, not the healer. The fix is two-part:
//!   1. The opportunistic placement (in `hunter.rs`) aims at the healer's
//!      POSITION, not the midpoint, and only fires when the healer (with no
//!      nearer enemy) will trigger it — otherwise it HOLDS the trap.
//!   2. This dip: when the off-target enemy healer is out of throw range and
//!      the Hunter is NOT the enemy's kill target (safe to reposition), the
//!      Hunter walks toward the healer until it is in range, then the ability
//!      pass places the trap on it.
//!
//! The dip is gated OFF while the Hunter is focused (walking into the enemy
//! backline while trained gets it killed) and aborts if it becomes focused
//! mid-walk, the healer becomes ineligible, or the budget expires.

use bevy::prelude::*;

use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::components::{
    ActiveAuras, Combatant, DRCategory, KitePosture, MovementDirective, MovementGoal,
};
use crate::states::play_match::decision_trace::{
    DecisionTrace, MovementGoalKind, MovementTrigger, Posture as TracePosture,
};
use crate::states::play_match::movement_config::DpsMovementConfig;

use super::cast_guard::{pre_cast_ok, PreCastOpts};
use super::healer_postures::start_movement_event_with_target;
use super::CombatContext;
use crate::states::play_match::constants::{
    TRAP_ARM_DELAY, TRAP_LAUNCH_MIN_RANGE, TRAP_LAUNCH_SPEED, TRAP_TRIGGER_RADIUS,
};

/// How close the Hunter walks to the enemy healer before dropping the trap. A
/// placed trap takes `TRAP_ARM_DELAY` (1.5s) to arm and triggers within
/// `TRAP_TRIGGER_RADIUS` (5yd), so lobbing it from max throw range lands it
/// where the (mobile) healer *was*. The dip closes to point-blank so the trap
/// drops on the healer and arms before it can walk clear of the trigger radius.
/// Also the threshold below which the opportunistic placement in `hunter.rs`
/// fires without a dip.
pub const HUNTER_TRAP_PLANT_RANGE: f32 = 8.0;

/// Plan handed from [`evaluate_hunter_dip`] to the Hunter ability pass.
#[derive(Clone, Copy, Debug, Default)]
pub enum HunterDipPlan {
    /// No dip — run the normal ENGAGE/KITE machine + ability rotation.
    #[default]
    Rotation,
    /// Dip live, walking toward the healer. The dip owns movement this frame
    /// (the caller skips `evaluate_dps_posture`); the ability pass still runs
    /// its normal rotation (instants fired while walking are fine).
    Walking,
    /// Arrived within throw range — the ability pass places Freezing Trap on
    /// `target` this GCD and installs `completed_state` (dip cleared) on success.
    DipCast {
        target: Entity,
        completed_state: KitePosture,
    },
}

impl HunterDipPlan {
    /// Does the dip own movement this frame (so the caller skips the ENGAGE/KITE
    /// evaluation)?
    pub fn owns_movement(self) -> bool {
        matches!(self, HunterDipPlan::Walking | HunterDipPlan::DipCast { .. })
    }
}

/// Is the Hunter the enemy team's kill target — focused by a real damage threat?
/// A healer wanding the Hunter does NOT count (it isn't a kill threat, and the
/// Hunter must stay free to dip past it); only a non-healer enemy targeting the
/// Hunter gates the dip off (and aborts a live one).
fn focused_by_threat(ctx: &CombatContext, entity: Entity) -> bool {
    ctx.enemies_targeting(entity)
        .iter()
        .any(|e| !e.class.is_healer())
}

/// The team's current kill targets: entities a living non-pet, non-healer ally
/// is targeting, excluding the Hunter itself. Healer allies are excluded because
/// a healer's `target` is an opportunistic wand/Mind-Blast mark, not a kill
/// commitment — counting it would wrongly mark the enemy healer as off-limits in
/// every healer mirror (making the trap fall back to the chaser). The Hunter's
/// OWN kill target is folded in by the caller.
fn team_focus(ctx: &CombatContext, entity: Entity) -> std::collections::BTreeSet<Entity> {
    ctx.alive_allies()
        .into_iter()
        .filter(|a| a.entity != entity && !a.is_pet && !a.class.is_healer())
        .filter_map(|a| a.target)
        .collect()
}

/// Is `target` a valid Freezing Trap off-target: an alive, visible, non-pet
/// enemy that is not immune, not incapacitate-DR-immune, and does not already
/// carry a friendly break-on-damage DoT (which would pop the trap on its first
/// tick). Class-agnostic — the trap CCs whoever the team is NOT killing
/// (usually the healer, but a DPS when the team focuses the healer).
fn dip_target_eligible(ctx: &CombatContext, my_team: u8, target: Entity) -> bool {
    let Some(info) = ctx.combatants.get(&target) else {
        return false;
    };
    info.team != my_team
        && info.is_alive
        && !info.is_pet
        && !ctx.entity_is_immune(target)
        && !ctx.is_dr_immune(target, DRCategory::Incapacitates)
        && !ctx.has_friendly_dots_on_target(target)
}

/// The units the Hunter's team is killing: [`team_focus`] plus the Hunter's own
/// kill target. The own-target fold is load-bearing in 1v1, where there are no
/// allies and so no `team_focus` at all.
pub fn trap_focus(
    ctx: &CombatContext,
    entity: Entity,
    own_target: Option<Entity>,
) -> std::collections::BTreeSet<Entity> {
    let mut focused = team_focus(ctx, entity);
    focused.extend(own_target);
    focused
}

/// Is a Freezing Trap on `victim` worth throwing, by who could undo it?
///
/// - A **healer** always is: trapping the dispeller is the point, and where a
///   second dispeller stands behind it — a Warlock's Felhunter, a second healer
///   — the Hunter still takes the healer and lets that dispeller spend itself
///   undoing it. That is the intended counter being PLAYED.
/// - Anyone else is worth it when no living teammate of theirs could free it
///   ([`ally_freers`](super::ally_freers), the engine's own removal rules) —
///   a trapped Felhunter included, which nobody can free —
/// - **or when every such teammate is one the Hunter's team is killing**
///   (`focused`). Freeing it then costs the focused dispeller a GCD it needed
///   for its own survival: the trap is a trade, not a spent GCD.
///
/// Otherwise the trap would be lifted for nothing — AS-68 measured the median
/// removal at 0.28s — and is held.
pub fn trap_victim_worth_it(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    owner: Entity,
    victim: Entity,
    focused: &std::collections::BTreeSet<Entity>,
) -> bool {
    let Some(info) = ctx.combatants.get(&victim) else {
        return false;
    };
    if !info.is_pet && info.class.is_healer() {
        return true;
    }
    let aura = crate::states::play_match::traps::freezing_trap_aura(owner);
    super::ally_freers(ctx, abilities, victim, &aura)
        .iter()
        .all(|freer| focused.contains(freer))
}

/// How long after a lane trap ARMS its victim must be predicted to reach it.
/// Motion is extrapolated from this frame's heading, and every unit re-decides
/// its path each tick, so a prediction further out is guesswork: a trap nobody
/// is predicted to reach within the window catches whoever wanders in later,
/// which is a decision nobody made.
pub const TRAP_SPRING_WINDOW: f32 = 3.0;

/// Sampling step of [`predicted_trap_springer`]'s extrapolation, in seconds.
const SPRING_PREDICTION_STEP: f32 = 0.05;

/// The enemy that would spring a trap thrown from `my_pos` to land at
/// `landing`, or `None` when no enemy is predicted to reach it in time.
///
/// A trap springs on the FIRST enemy inside its trigger radius once it has
/// ARMED (launch travel, then `TRAP_ARM_DELAY`), so the victim is not whoever
/// stands nearest the landing now but whoever is inside the radius first after
/// arming. Each visible living enemy, pets included, is extrapolated along its
/// estimated [`velocity`](super::CombatantInfo::velocity) (zero while casting),
/// stopping at its preferred range of its own target when it is heading toward
/// it — a pursuer does not run through the unit it is chasing, and a caster
/// stops at casting range. The springer is the
/// one inside the radius earliest in `[armed, armed + TRAP_SPRING_WINDOW]`
/// (ties: nearest the landing, then entity order). An enemy that crosses the
/// landing before it arms and runs on is not a springer.
///
/// Only enemies the Hunter can see are candidates;
/// [`unseen_victim_would_be_freed`] answers for the rest.
pub fn predicted_trap_springer(
    ctx: &CombatContext,
    my_team: u8,
    my_pos: Vec3,
    landing: Vec3,
) -> Option<Entity> {
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let (landing, my_pos) = (flat(landing), flat(my_pos));
    let throw = my_pos.distance(landing);
    let travel = if throw < TRAP_LAUNCH_MIN_RANGE {
        0.0
    } else {
        throw / TRAP_LAUNCH_SPEED
    };
    let armed = travel + TRAP_ARM_DELAY;

    let mut best: Option<(f32, f32, Entity)> = None;
    for enemy in ctx
        .combatants
        .values()
        .filter(|e| e.team != my_team && e.is_alive)
    {
        let start = flat(enemy.position);
        let velocity = flat(enemy.velocity);
        let speed = velocity.length();
        let heading = velocity.normalize_or_zero();
        // A pursuer stops at its preferred range of its target, as pursuit in
        // `move_to_target` does: the closest approach along its heading, less
        // that range. Heading away (a kiter), or with no target in view, it
        // keeps going.
        let stop = match enemy.pet_type {
            Some(pet_type) => pet_type.preferred_range(),
            None => enemy.class.preferred_range(),
        };
        let reach = enemy
            .target
            .and_then(|t| ctx.combatants.get(&t))
            .map(|t| (flat(t.position) - start).dot(heading))
            .filter(|along| *along > 0.0)
            .map_or(f32::INFINITY, |along| (along - stop).max(0.0));
        let steps = (TRAP_SPRING_WINDOW / SPRING_PREDICTION_STEP).round() as usize;
        let first_inside = (0..=steps)
            .map(|i| armed + i as f32 * SPRING_PREDICTION_STEP)
            .find(|t| {
                let at = start + heading * (speed * t).min(reach);
                at.distance(landing) <= TRAP_TRIGGER_RADIUS
            });
        if let Some(t) = first_inside {
            let key = (t, start.distance(landing), enemy.entity);
            if best.is_none_or(|b| (key.0, key.1) < (b.0, b.1)) {
                best = Some(key);
            }
        }
    }
    best.map(|(_, _, e)| e)
}

/// Would the Hunter's own team break a Freezing Trap on `victim`? The trap
/// breaks on any damage, and a teammate does not hold fire for it: true when a
/// living non-pet teammate targets `victim`, or when `victim` is the Hunter's
/// own kill target (`own_target`) and the Hunter has a living teammate at all
/// — the team converges on it. Healers count: a wand or a Mind Blast breaks it
/// as surely as a Mortal Strike. Alone, the Hunter and its pet hold fire on a
/// trapped target, so in 1v1 this is always false.
pub fn teammate_would_break_it(
    ctx: &CombatContext,
    entity: Entity,
    own_target: Option<Entity>,
    victim: Entity,
) -> bool {
    let teammates: Vec<_> = ctx
        .alive_allies()
        .into_iter()
        .filter(|a| a.entity != entity)
        .collect();
    !teammates.is_empty()
        && (own_target == Some(victim) || teammates.iter().any(|a| a.target == Some(victim)))
}

/// Could a trap thrown into open ground be sprung by an enemy the Hunter
/// cannot see, and that enemy then be freed? True while an enemy is hidden
/// ([`CombatContext::enemy_hidden`]) and a visible enemy's kit frees a
/// teammate from the trap ([`frees_teammates`](super::frees_teammates)) —
/// asked of the dispeller, because the victim is not in view: at gates-open
/// the enemy Rogue is stealthed, runs into the lane, and its Priest frees it.
pub fn unseen_victim_would_be_freed(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    owner: Entity,
    my_team: u8,
) -> bool {
    if !ctx.enemy_hidden() {
        return false;
    }
    let aura = crate::states::play_match::traps::freezing_trap_aura(owner);
    ctx.combatants
        .values()
        .any(|e| e.team != my_team && super::frees_teammates(abilities, e, &aura))
}

/// Best eligible OFF-target enemy within `reach` that the team is NOT already
/// killing (so the trap CCs the off-target, not the kill target), and whose trap
/// is worth throwing ([`trap_victim_worth_it`]). Prefers a healer (highest CC
/// value — shut down enemy sustain, and the dispeller itself) and breaks ties
/// by nearest. `reach` may be `f32::INFINITY` for the no-walk opportunistic
/// path.
pub fn dip_target_candidate(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    owner: Entity,
    my_team: u8,
    my_pos: Vec3,
    reach: f32,
    focused: &std::collections::BTreeSet<Entity>,
) -> Option<Entity> {
    ctx.alive_enemies()
        .into_iter()
        .filter(|e| !e.is_pet)
        .filter(|e| !focused.contains(&e.entity))
        .filter(|e| dip_target_eligible(ctx, my_team, e.entity))
        .filter(|e| my_pos.distance(e.position) <= reach)
        .filter(|e| trap_victim_worth_it(ctx, abilities, owner, e.entity, focused))
        .min_by(|a, b| {
            // Healers first (`!is_healer` is false=0 for healers, sorts first),
            // then nearest.
            (!a.class.is_healer()).cmp(&!b.class.is_healer()).then(
                my_pos
                    .distance(a.position)
                    .partial_cmp(&my_pos.distance(b.position))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        })
        .map(|e| e.entity)
}

/// The off-target candidate for the no-walk opportunistic placement: no reach
/// limit, excluding `focused` ([`trap_focus`]). Returns `(entity, position)`.
pub fn opportunistic_off_target(
    ctx: &CombatContext,
    abilities: &AbilityDefinitions,
    entity: Entity,
    my_team: u8,
    focused: &std::collections::BTreeSet<Entity>,
    my_pos: Vec3,
) -> Option<(Entity, Vec3)> {
    let target = dip_target_candidate(
        ctx,
        abilities,
        entity,
        my_team,
        my_pos,
        f32::INFINITY,
        focused,
    )?;
    ctx.combatants
        .get(&target)
        .filter(|i| i.is_alive)
        .map(|i| (target, i.position))
}

/// Evaluate the Hunter Freezing Trap dip. Runs BEFORE the ENGAGE/KITE machine;
/// when it owns movement the caller skips `evaluate_dps_posture` so the dip
/// directive isn't overwritten by a kite vector. Returns the plan the ability
/// pass consumes.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_hunter_dip(
    commands: &mut Commands,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    posture: Option<&mut KitePosture>,
    directive: Option<&MovementDirective>,
    config: &DpsMovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
) -> HunterDipPlan {
    // Dip disabled (Mage / config 0), or no persistent state yet (frame 1
    // before the KITE machine inserts KitePosture) → no dip this frame.
    if config.dip_budget <= 0.0 {
        return HunterDipPlan::Rotation;
    }
    let Some(state) = posture else {
        return HunterDipPlan::Rotation;
    };

    let def = abilities.get_unchecked(&AbilityType::FreezingTrap);
    let trap_ready = pre_cast_ok(
        AbilityType::FreezingTrap,
        def,
        combatant,
        my_pos,
        auras,
        None,
        ctx,
        PreCastOpts::default(),
    );

    // ---- LIVE DIP ----
    if state.dipping(now) {
        let target = state.dip_target.expect("dipping() guarantees Some target");

        // Abort: target no longer trap-eligible, OR the Hunter became the
        // enemy's focus (walking into the backline while trained gets it
        // killed), OR Freezing Trap stopped being castable (mana / CD / lock).
        let became_focused = focused_by_threat(ctx, entity);
        if became_focused || !trap_ready || !dip_target_eligible(ctx, combatant.team, target) {
            state.dip_target = None;
            state.dip_until = 0.0;
            if directive.is_some() {
                commands.entity(entity).remove::<MovementDirective>();
            }
            emit_dip(
                decision_trace,
                ctx,
                target,
                my_pos,
                MovementTrigger::DipAbort,
            );
            return HunterDipPlan::Rotation;
        }

        // Arrival: within point-blank plant range → command the cast.
        let plant_close = ctx
            .combatants
            .get(&target)
            .is_some_and(|t| my_pos.distance(t.position) <= HUNTER_TRAP_PLANT_RANGE);
        if plant_close {
            let mut completed = *state;
            completed.dip_target = None;
            completed.dip_until = 0.0;
            return HunterDipPlan::DipCast {
                target,
                completed_state: completed,
            };
        }

        // Still walking: (re)issue the chase directive.
        commands.entity(entity).try_insert(MovementDirective {
            goal: MovementGoal::Entity(target),
            expires: state.dip_until,
            committed_until: state.dip_until,
        });
        return HunterDipPlan::Walking;
    }

    // ---- ENTRY ----
    // Only dip when safe: not currently focused by an enemy damage threat, and
    // the trap is ready to spend.
    if focused_by_threat(ctx, entity) || !trap_ready {
        return HunterDipPlan::Rotation;
    }

    // Reach: how far we'll commit to walk to get point-blank within the budget.
    let reach = HUNTER_TRAP_PLANT_RANGE
        + config.dip_budget * combatant.base_movement_speed * ctx.movement_slow_multiplier(entity);
    // Exclude the team's kill targets AND the Hunter's own kill target: the trap
    // CCs the off-target, never the target being killed. The own-target guard is
    // load-bearing in 1v1, where there are no allies (empty `team_focus`) — else
    // the Hunter would dip into its own target (e.g. point-blank onto a Warlock).
    let focused = trap_focus(ctx, entity, combatant.target);
    let Some(target) = dip_target_candidate(
        ctx,
        abilities,
        entity,
        combatant.team,
        my_pos,
        reach,
        &focused,
    ) else {
        return HunterDipPlan::Rotation;
    };

    // Already point-blank → no dip needed; the opportunistic placement in the
    // ability pass drops it on the healer this tick.
    let plant_close = ctx
        .combatants
        .get(&target)
        .is_some_and(|t| my_pos.distance(t.position) <= HUNTER_TRAP_PLANT_RANGE);
    if plant_close {
        return HunterDipPlan::Rotation;
    }

    // Within reach but not point-blank, and safe → start the dip.
    state.dip_target = Some(target);
    state.dip_until = now + config.dip_budget;
    commands.entity(entity).try_insert(MovementDirective {
        goal: MovementGoal::Entity(target),
        expires: state.dip_until,
        committed_until: state.dip_until,
    });
    emit_dip(
        decision_trace,
        ctx,
        target,
        my_pos,
        MovementTrigger::DipEnter,
    );
    HunterDipPlan::Walking
}

/// Where to aim a Freezing Trap so it lands on — or just ahead of — `target`
/// when it arms. A planted target (casting → zero `velocity`) is aimed at
/// directly; a moving target is led along its velocity by the trap's travel +
/// arm time so the trap drops into its path. The lead is capped so a long arm
/// window can't fling the aim across the arena when the target turns.
pub fn trap_lead_landing(ctx: &CombatContext, target: Entity, my_pos: Vec3) -> Option<Vec3> {
    let info = ctx.combatants.get(&target)?;
    let travel = my_pos.distance(info.position) / TRAP_LAUNCH_SPEED;
    let lead_time = TRAP_ARM_DELAY + travel;
    let lead = (info.velocity * lead_time).clamp_length_max(TRAP_TRIGGER_RADIUS * 3.0);
    Some(info.position + lead)
}

/// Emit the `DipComplete` movement event after the ability pass lands the trap.
/// Separate entry point so `hunter.rs` can fire it once the cast actually
/// succeeds (the dip enters/aborts here, but completes there).
pub fn emit_dip_complete(
    decision_trace: &mut DecisionTrace,
    ctx: &CombatContext,
    target: Entity,
    my_pos: Vec3,
) {
    emit_dip(
        decision_trace,
        ctx,
        target,
        my_pos,
        MovementTrigger::DipComplete,
    );
}

fn emit_dip(
    decision_trace: &mut DecisionTrace,
    ctx: &CombatContext,
    target: Entity,
    my_pos: Vec3,
    trigger: MovementTrigger,
) {
    let (from, to) = match trigger {
        MovementTrigger::DipEnter => (TracePosture::Engage, TracePosture::Dip),
        _ => (TracePosture::Dip, TracePosture::Engage),
    };
    if let Some(mut builder) = start_movement_event_with_target(decision_trace, ctx, target, my_pos)
    {
        builder.transition(from, to, trigger, MovementGoalKind::Entity);
        builder.finish();
    }
}
