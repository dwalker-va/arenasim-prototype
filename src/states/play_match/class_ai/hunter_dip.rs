//! Hunter Freezing Trap "dip" — a committed walk to set a Freezing Trap on the
//! enemy healer, mirroring the Paladin HoJ dip and Priest Scream dip.
//!
//! ## Why
//! A placed trap triggers on the FIRST enemy inside its radius (`traps.rs`), so
//! a Freezing Trap thrown at the Hunter↔healer midpoint while a melee chases is
//! eaten by the chaser, not the healer. The fix is two-part:
//!   1. While such an off-target healer exists, the rotation (in `hunter.rs`)
//!      HOLDS the trap rather than lob it into the lane.
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
use crate::states::play_match::ability_book::AbilityBook;
use crate::states::play_match::components::{
    ActiveAuras, Combatant, DRCategory, KitePosture, MovementDirective, MovementGoal, TrapRetarget,
};
use crate::states::play_match::decision_trace::{
    DecisionTrace, MovementGoalKind, MovementTrigger, Posture as TracePosture,
};
use crate::states::play_match::movement_config::DpsMovementConfig;

use super::cast_guard::{pre_cast_ok, PreCastOpts};
use super::dps_postures::TrapSetup;
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
/// Also the threshold within which no dip starts: the healer is already
/// point-blank.
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
        /// The trap walk's throw (AS-498): the enemy the Hunter fights while
        /// the trap holds `target` ([`TrapRetarget`]), and the landing is the
        /// pressure trap's clean one. `None` for the old dip's point-blank
        /// drop.
        hold: Option<Entity>,
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
    ctx.enemies_targeting(entity).iter().any(|e| !e.is_healer())
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
        .filter(|a| a.entity != entity && !a.is_pet && !a.is_healer())
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
/// - Anyone else is worth it only when it would stay trapped
///   ([`trap_would_stick`]) — a pet included: a healer's dispel frees a
///   trapped pet as it frees any teammate (AS-129).
///
/// Otherwise the trap would be lifted for nothing — AS-68 measured the median
/// removal at 0.28s — and is held. That holds even when the only teammate who
/// could free it is one the Hunter's team is killing: a focused Priest still
/// dispels its partner within a GCD or two, so such a trap buys nothing.
pub fn trap_victim_worth_it(ctx: &CombatContext, owner: Entity, victim: Entity) -> bool {
    let Some(info) = ctx.combatants.get(&victim) else {
        return false;
    };
    if !info.is_pet && info.is_healer() {
        return true;
    }
    trap_would_stick(ctx, owner, victim)
}

/// Would a Freezing Trap on `victim` stay on it? True when no living teammate
/// of `victim` could free it ([`ally_freers`](super::ally_freers), the engine's
/// own removal rules).
///
/// A Paladin's own Divine Shield is not asked: the bubble lifts a trap only
/// when the Paladin's AI chooses to spend a 5-minute cooldown on it (a teammate
/// low, or itself), and a trap that draws the bubble has spent the Paladin's
/// one emergency button — a trade the Hunter takes.
pub fn trap_would_stick(ctx: &CombatContext, owner: Entity, victim: Entity) -> bool {
    let aura = crate::states::play_match::traps::freezing_trap_aura(owner);
    super::ally_freers(ctx, victim, &aura).is_empty()
}

/// The enemy healer to Freezing Trap while a melee is on the Hunter — the
/// "trap the dispeller" ruling played under pressure. The trap goes on the
/// HEALER, not the melee: with its healer frozen, the melee running the Hunter
/// down is unhealed for the trap's duration.
///
/// Who, not where — the landing is checked at the throw. An enemy healer that:
///
/// - is trap-eligible (alive, not immune, not incapacitate-DR-immune, no
///   friendly DoT to break it on its first tick);
/// - **would not be freed** ([`trap_would_stick`]) — a Rogue or Warrior
///   partner cannot free it, a Felhunter or a second healer can;
/// - is not the target of a teammate who is killing it ([`team_focus`]):
///   every damage source holds fire on a trapped enemy, so trapping it would
///   idle that teammate for the trap's duration. A partner **healer** does not
///   hold it, although its occasional Mind Blast or wand shot can break the
///   trap ([`teammate_would_break_it`] counts healers, for the lane throw): a
///   healer's target is wherever acquisition left it, usually the enemy
///   healer, so counting it would hold this trap in nearly every Hunter +
///   healer team — measured in the AS-125 findings doc. The Hunter's OWN
///   target does not hold it either: the Hunter moves off the healer when it
///   throws ([`TrapRetarget`]).
///
/// `None` when no enemy healer is in view; otherwise the first eligible
/// healer, or why the last one considered was passed over.
pub fn pressure_trap_healer(
    ctx: &CombatContext,
    entity: Entity,
    my_team: u8,
) -> Option<Result<Entity, &'static str>> {
    let mut verdict = None;
    for healer in ctx
        .alive_enemies()
        .into_iter()
        .filter(|e| !e.is_pet && e.is_healer())
    {
        let result = if !dip_target_eligible(ctx, my_team, healer.entity) {
            Err(PRESSURE_TRAP_INELIGIBLE)
        } else if team_focus(ctx, entity).contains(&healer.entity) {
            Err(PRESSURE_TRAP_TEAM_ATTACKING)
        } else if !trap_would_stick(ctx, entity, healer.entity) {
            Err(PRESSURE_TRAP_FREEABLE)
        } else {
            Ok(healer.entity)
        };
        verdict = Some(result);
        if result.is_ok() {
            break;
        }
    }
    verdict
}

/// Where a pressure trap on `healer` would land from `my_pos`, when it would
/// catch it cleanly: its led landing ([`trap_lead_landing`]) within the trap's
/// configured range, `healer` predicted to spring it
/// ([`predicted_trap_springer`]) and no other enemy near the landing now.
/// `Err` names the gate that held the throw: the healer itself beyond range
/// ([`PRESSURE_TRAP_OUT_OF_RANGE`]) is told apart from a healer in range whose
/// lead carries the landing beyond it ([`PRESSURE_TRAP_LEAD_OUT_OF_RANGE`]) —
/// a healer walking away is led by up to three trigger radii.
pub fn pressure_trap_landing(
    ctx: &CombatContext,
    abilities: &AbilityBook,
    my_team: u8,
    my_pos: Vec3,
    healer: Entity,
) -> Result<Vec3, &'static str> {
    let landing = trap_lead_landing(ctx, healer, my_pos).ok_or(PRESSURE_TRAP_NO_HEALER)?;
    let range = abilities.get_unchecked(&AbilityType::FreezingTrap).range;
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    if flat(my_pos).distance(flat(landing)) > range {
        let healer_at = ctx.combatants.get(&healer).map_or(landing, |h| h.position);
        return Err(if flat(my_pos).distance(flat(healer_at)) > range {
            PRESSURE_TRAP_OUT_OF_RANGE
        } else {
            PRESSURE_TRAP_LEAD_OUT_OF_RANGE
        });
    }
    let crowded = ctx.combatants.values().any(|other| {
        other.team != my_team
            && other.is_alive
            && other.entity != healer
            && other.position.distance(landing) <= TRAP_TRIGGER_RADIUS
    });
    if crowded || predicted_trap_springer(ctx, my_team, my_pos, landing) != Some(healer) {
        return Err(PRESSURE_TRAP_NOT_CLEAN);
    }
    Ok(landing)
}

/// The target hold a pressure trap on `healer` places on the Hunter, handed
/// to target acquisition (`acquire_targets`): while it lasts the Hunter's
/// target is `melee`, never `healer` — every shot holds fire on a trapped
/// enemy, so staying on the healer would idle the Hunter for the trap's
/// duration, and a configured kill target re-forced onto the healer would do
/// exactly that. It lasts until `healer` is dead, or until the trap was
/// predicted to have sprung ([`trap_spring_deadline`]) and `healer` carries no
/// Freezing Trap of this Hunter's.
pub fn pressure_trap_hold(
    my_pos: Vec3,
    landing: Vec3,
    healer: Entity,
    melee: Entity,
    now: f32,
) -> TrapRetarget {
    TrapRetarget {
        healer,
        melee,
        spring_by: now + trap_spring_deadline(my_pos, landing),
    }
}

/// The enemy healer the Hunter's kite should keep within throw range, or
/// `None`: Freezing Trap is ready to cast, and a [`pressure_trap_healer`]
/// exists. Movement is unchanged whenever this is `None` — in particular while
/// the trap is on cooldown.
pub fn trap_setup_healer(
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) -> Option<Entity> {
    let def = abilities.get_unchecked(&AbilityType::FreezingTrap);
    let ready = pre_cast_ok(
        AbilityType::FreezingTrap,
        def,
        combatant,
        my_pos,
        auras,
        None,
        ctx,
        PreCastOpts::default(),
    );
    if !ready {
        return None;
    }
    pressure_trap_healer(ctx, entity, combatant.team)?.ok()
}

/// How close to the healer the Hunter repositions to trap it: the trap's
/// configured range, less the trigger radius the lead landing may carry it.
pub fn trap_setup_range(abilities: &AbilityBook) -> f32 {
    abilities.get_unchecked(&AbilityType::FreezingTrap).range - TRAP_TRIGGER_RADIUS
}

/// The [`TrapSetup`] the Hunter's KITE scores against: the
/// [`trap_setup_healer`]'s position and [`trap_setup_range`], or `None`.
pub fn trap_setup(
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
) -> Option<TrapSetup> {
    let healer = trap_setup_healer(abilities, entity, combatant, my_pos, auras, ctx)?;
    Some(TrapSetup {
        healer: ctx.combatants.get(&healer)?.position,
        range: trap_setup_range(abilities),
    })
}

/// Disengage's leap direction with the trap setup folded in: straight `away`
/// from the melee, bent toward `healer` when the straight leap would land
/// beyond `range` of it. Weighted as the KITE scorer weighs the same two pulls
/// (`flee` away, `trap_setup` toward), so the leap and the kite agree; with
/// `trap_setup` below `flee` the leap always still carries the Hunter away
/// from the melee. Planar; `away` is returned unchanged when there is nothing
/// to bend toward.
pub fn trap_setup_disengage(
    away: Vec3,
    my_pos: Vec3,
    leap: f32,
    healer: Vec3,
    range: f32,
    flee: f32,
    trap_setup: f32,
) -> Vec3 {
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let straight = flat(my_pos) + flat(away) * leap;
    if trap_setup <= 0.0 || straight.distance(flat(healer)) <= range {
        return away;
    }
    let toward = (flat(healer) - flat(my_pos)).normalize_or_zero();
    let bent = (flat(away) * flee + toward * trap_setup).normalize_or_zero();
    if bent.dot(away) > 0.0 {
        bent
    } else {
        away
    }
}

/// Trace note: a pressure trap held because the healer is immune, DR-immune
/// to incapacitates, or carries a friendly DoT that would break it.
pub const PRESSURE_TRAP_INELIGIBLE: &str = "healer trap held: the healer cannot be trapped now";

/// Trace note: a pressure trap held because a teammate of the Hunter's is
/// killing the healer.
pub const PRESSURE_TRAP_TEAM_ATTACKING: &str = "healer trap held: a teammate is killing the healer";

/// Trace note: a pressure trap held because a teammate of the healer's would
/// free it.
pub const PRESSURE_TRAP_FREEABLE: &str = "healer trap held: a teammate would free the healer";

/// Trace note: a pressure trap held because the healer itself is beyond the
/// trap's configured range.
pub const PRESSURE_TRAP_OUT_OF_RANGE: &str = "healer trap held: the healer is out of throw range";

/// Trace note: a pressure trap held because the healer is within throw range
/// but its lead landing, ahead of it along its heading, is not.
pub const PRESSURE_TRAP_LEAD_OUT_OF_RANGE: &str =
    "healer trap held: the healer's lead lands out of throw range";

/// Trace note: a pressure trap held because the healer is not the enemy that
/// would spring it — another enemy is near the landing, or it would not reach
/// the landing once the trap arms.
pub const PRESSURE_TRAP_NOT_CLEAN: &str =
    "healer trap held: the healer would not spring it cleanly";

/// Trace note: the healer left the Hunter's view between choosing and landing.
const PRESSURE_TRAP_NO_HEALER: &str = "healer trap held: the healer is not in view";

/// How long after a lane trap ARMS its victim must be predicted to reach it.
/// Motion is extrapolated from this frame's heading, and every unit re-decides
/// its path each tick, so a prediction further out is guesswork: a trap nobody
/// is predicted to reach within the window catches whoever wanders in later,
/// which is a decision nobody made.
pub const TRAP_SPRING_WINDOW: f32 = 3.0;

/// Sampling step of [`predicted_trap_springer`]'s extrapolation, in seconds.
const SPRING_PREDICTION_STEP: f32 = 0.05;

/// Seconds from a throw at `my_pos` until the trap landing at `landing` arms:
/// its launch travel (none for a drop inside `TRAP_LAUNCH_MIN_RANGE`), then
/// `TRAP_ARM_DELAY`. Planar.
pub fn trap_armed_after(my_pos: Vec3, landing: Vec3) -> f32 {
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    let throw = flat(my_pos).distance(flat(landing));
    let travel = if throw < TRAP_LAUNCH_MIN_RANGE {
        0.0
    } else {
        throw / TRAP_LAUNCH_SPEED
    };
    travel + TRAP_ARM_DELAY
}

/// Seconds from a throw until the last moment [`predicted_trap_springer`]
/// looks at: the trap arms ([`trap_armed_after`]), then
/// [`TRAP_SPRING_WINDOW`]. A trap not sprung by then was not sprung by the
/// enemy it was decided on.
pub fn trap_spring_deadline(my_pos: Vec3, landing: Vec3) -> f32 {
    trap_armed_after(my_pos, landing) + TRAP_SPRING_WINDOW
}

/// The enemy that would spring a trap thrown from `my_pos` to land at
/// `landing`, or `None` when no enemy is predicted to reach it in time.
///
/// A trap springs on the FIRST enemy inside its trigger radius once it has
/// ARMED (launch travel, then `TRAP_ARM_DELAY`), so the victim is not whoever
/// stands nearest the landing now but whoever is inside the radius first after
/// arming. Each visible living enemy, pets included, is extrapolated along its
/// [`forecast_velocity`] (zero while casting, rooted or held; slowed by its
/// slows), stopping at its preferred range of its own target when it is heading toward
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
    let armed = trap_armed_after(my_pos, landing);
    let landing = flat(landing);

    let mut best: Option<(f32, f32, Entity)> = None;
    for enemy in ctx
        .combatants
        .values()
        .filter(|e| e.team != my_team && e.is_alive)
    {
        if let Some(t) = first_inside(ctx, enemy, landing, armed, TRAP_SPRING_WINDOW) {
            let key = (t, flat(enemy.position).distance(landing), enemy.entity);
            if best.is_none_or(|b| (key.0, key.1) < (b.0, b.1)) {
                best = Some(key);
            }
        }
    }
    best.map(|(_, _, e)| e)
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// The first moment in `[from, from + window]` (seconds from now, sampled at
/// `SPRING_PREDICTION_STEP`) at which `enemy`, extrapolated along its
/// [`forecast_velocity`] (the one the trap's lead is aimed by: slowed by its
/// slows, still while rooted or held), is inside the trigger radius of a trap
/// at `landing` (planar). A pursuer stops at its
/// preferred range of its own target, as pursuit in `move_to_target` does: the
/// closest approach along its heading, less that range. Heading away (a
/// kiter), or with no target in view, it keeps going.
pub(super) fn first_inside(
    ctx: &CombatContext,
    enemy: &super::CombatantInfo,
    landing: Vec3,
    from: f32,
    window: f32,
) -> Option<f32> {
    let start = flat(enemy.position);
    let velocity = flat(forecast_velocity(ctx, enemy.entity));
    let speed = velocity.length();
    let heading = velocity.normalize_or_zero();
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
    let steps = (window / SPRING_PREDICTION_STEP).round() as usize;
    (0..=steps)
        .map(|i| from + i as f32 * SPRING_PREDICTION_STEP)
        .find(|t| {
            let at = start + heading * (speed * t).min(reach);
            at.distance(landing) <= TRAP_TRIGGER_RADIUS
        })
}

/// One of the Hunter's own Freezing Traps still waiting to spring — in flight
/// or on the ground — as the ability AI sees it: where it lands and how many
/// seconds until it arms (`0.0` once armed). Built each frame from the live
/// `Trap` / `TrapLaunchProjectile` entities, so it is never stale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveTrap {
    pub position: Vec3,
    pub armed_in: f32,
}

/// Would one of the Hunter's own live Freezing Traps catch `target` before
/// damage the Hunter starts now lands on it, `lands_after` seconds from now?
/// The trap breaks on any damage, so that damage would free the enemy it just
/// froze: an Aimed Shot begun as the enemy runs into the trap lands a moment
/// after it springs. True when `target`, extrapolated as
/// [`predicted_trap_springer`] extrapolates a lane throw's springer, is inside
/// a trap's trigger radius at some moment between the trap arming and the
/// damage landing. Damage that lands before a trap arms cannot break it.
/// (An enemy the trap already holds is the `pre_cast_ok` friendly-CC guard's.)
pub fn own_trap_catches_first(
    ctx: &CombatContext,
    own_traps: &[LiveTrap],
    target: Entity,
    lands_after: f32,
) -> bool {
    let Some(enemy) = ctx.combatants.get(&target).filter(|e| e.is_alive) else {
        return false;
    };
    own_traps.iter().any(|trap| {
        let from = trap.armed_in.max(0.0);
        lands_after > from
            && first_inside(ctx, enemy, flat(trap.position), from, lands_after - from).is_some()
    })
}

/// Trace note: a damaging shot held because the Hunter's own Freezing Trap
/// would catch its target before it lands ([`own_trap_catches_first`]).
pub const OWN_TRAP_WOULD_BREAK: &str = "own Freezing Trap would catch the target before it lands";

/// Would the Hunter's own team break a Freezing Trap on `victim`? The trap
/// breaks on any damage, and a teammate does not hold fire for it: true when a
/// living teammate targets `victim` ([`teammate_targets`]), or when `victim` is
/// the Hunter's own kill target (`own_target`) and the Hunter has a living
/// teammate at all — the team converges on it. Alone, the Hunter and its pet
/// hold fire on a trapped target, so in 1v1 this is always false.
pub fn teammate_would_break_it(
    ctx: &CombatContext,
    entity: Entity,
    own_target: Option<Entity>,
    victim: Entity,
) -> bool {
    let has_teammate = ctx
        .alive_allies()
        .into_iter()
        .any(|a| a.entity != entity && !a.is_pet);
    teammate_targets(ctx, entity, victim) || (has_teammate && own_target == Some(victim))
}

/// Does a living non-pet teammate of the Hunter's target `victim`? Healers
/// count: a wand or a Mind Blast breaks a trap as surely as a Mortal Strike.
pub fn teammate_targets(ctx: &CombatContext, entity: Entity, victim: Entity) -> bool {
    ctx.alive_allies()
        .into_iter()
        .any(|a| a.entity != entity && !a.is_pet && a.target == Some(victim))
}

/// Could a trap thrown into open ground be sprung by an enemy the Hunter
/// cannot see, and that enemy then be freed? True while an enemy is hidden
/// ([`CombatContext::enemy_hidden`]) and a visible enemy's kit frees a
/// teammate from the trap ([`frees_teammates`](super::frees_teammates)) —
/// asked of the dispeller, because the victim is not in view: at gates-open
/// the enemy Rogue is stealthed, runs into the lane, and its Priest frees it.
pub fn unseen_victim_would_be_freed(ctx: &CombatContext, owner: Entity, my_team: u8) -> bool {
    if !ctx.enemy_hidden() {
        return false;
    }
    let aura = crate::states::play_match::traps::freezing_trap_aura(owner);
    ctx.combatants
        .values()
        .any(|e| e.team != my_team && super::frees_teammates(e, &aura))
}

/// Best eligible OFF-target enemy within `reach` that the team is NOT already
/// killing (so the trap CCs the off-target, not the kill target), and whose trap
/// is worth throwing ([`trap_victim_worth_it`]). Prefers a healer (highest CC
/// value — shut down enemy sustain, and the dispeller itself) and breaks ties
/// by nearest. `reach` may be `f32::INFINITY` for the off-target the
/// rotation holds the trap for.
pub fn dip_target_candidate(
    ctx: &CombatContext,
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
        .filter(|e| trap_victim_worth_it(ctx, owner, e.entity))
        .min_by(|a, b| {
            // Healers first (`!is_healer` is false=0 for healers, sorts first),
            // then nearest.
            (!a.is_healer()).cmp(&!b.is_healer()).then(
                my_pos
                    .distance(a.position)
                    .partial_cmp(&my_pos.distance(b.position))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        })
        .map(|e| e.entity)
}

/// The off-target the rotation holds the trap for (the dip throws it): no reach
/// limit, excluding `focused` ([`trap_focus`]). Returns `(entity, position)`.
pub fn opportunistic_off_target(
    ctx: &CombatContext,
    entity: Entity,
    my_team: u8,
    focused: &std::collections::BTreeSet<Entity>,
    my_pos: Vec3,
) -> Option<(Entity, Vec3)> {
    let target = dip_target_candidate(ctx, entity, my_team, my_pos, f32::INFINITY, focused)?;
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
    abilities: &AbilityBook,
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
    if config.trap_walk_worth >= 0.0 {
        return evaluate_trap_walk(
            commands,
            abilities,
            entity,
            combatant,
            my_pos,
            auras,
            ctx,
            state,
            directive,
            config,
            now,
            decision_trace,
        );
    }

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
                hold: None,
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
    let Some(target) = dip_target_candidate(ctx, entity, combatant.team, my_pos, reach, &focused)
    else {
        return HunterDipPlan::Rotation;
    };

    // Already point-blank → no dip needed. (A healer this near puts the Hunter
    // in its dead-zone block, where the pressure trap takes it.)
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

// ============================================================================
// AS-498 prototype: the trap walk
// ============================================================================

/// Seconds an enemy melee stays on the Hunter once a walk has let it reach
/// striking range: the time the Hunter needs to open the gap again (a
/// Disengage or a Concussive Shot on its next GCDs). Provisional.
pub const TRAP_WALK_STICK_SECS: f32 = 2.0;

/// Within this of the Hunter an enemy melee or pet strikes it (melee range
/// plus the slack its pursuit stops at).
const TRAP_WALK_CONTACT: f32 = 5.0;

/// Sampling step of [`walk_exposure`], in seconds.
const TRAP_WALK_STEP: f32 = 0.1;

/// Who the trap play freezes, and whom the Hunter fights while it holds:
/// `(victim, instead)`.
///
/// - **The enemy healer** when the pressure trap would take it
///   ([`pressure_trap_healer`]: eligible, not a teammate's kill, and no
///   teammate of its could free it — `ally_freers`). The Hunter fights the
///   nearest melee threat instead, else the nearest other enemy it sees; with
///   nobody else to fight (1v1) there is no trap play.
/// - **The outlast trap** when the team is killing the enemy healer
///   ([`team_focus`] holds it): the gameplan is to drain it, so the trap goes
///   on an enemy DPS that would stay frozen ([`trap_would_stick`], the same
///   `ally_freers` rule), no teammate of the Hunter's is attacking it, and it
///   is eligible — the one targeting the Hunter first, then the nearest. The
///   Hunter fights the healer instead.
///
/// `spare` (the Hunter's own kill target, when
/// `hunter.trap_walk_spare_own_target` is on) is never the healer victim.
///
/// With `needs_teammate`, the healer is taken only while a DPS teammate is
/// attacking someone ([`team_focus`] not empty).
///
/// `None` otherwise: the trap is not the trap play's to throw.
pub fn trap_play_victim(
    ctx: &CombatContext,
    entity: Entity,
    my_team: u8,
    my_pos: Vec3,
    spare: Option<Entity>,
    needs_teammate: bool,
) -> Option<(Entity, Entity)> {
    if let Some(Ok(healer)) = pressure_trap_healer(ctx, entity, my_team)
        .filter(|v| spare.is_none() || v.ok() != spare)
        .filter(|_| !needs_teammate || !team_focus(ctx, entity).is_empty())
    {
        let instead = super::dps_postures::nearest_melee_threat_where(ctx, entity, my_pos, |e| {
            e != healer && !ctx.entity_is_immune(e)
        })
        .map(|(e, _)| e)
        .or_else(|| {
            ctx.alive_enemies()
                .into_iter()
                .filter(|e| !e.is_pet && e.entity != healer && !ctx.entity_is_immune(e.entity))
                .min_by(|a, b| {
                    a.position
                        .distance(my_pos)
                        .total_cmp(&b.position.distance(my_pos))
                })
                .map(|e| e.entity)
        })?;
        return Some((healer, instead));
    }
    let focus = team_focus(ctx, entity);
    let drained = ctx
        .alive_enemies()
        .into_iter()
        .find(|e| !e.is_pet && e.is_healer() && focus.contains(&e.entity))?
        .entity;
    let victim = ctx
        .alive_enemies()
        .into_iter()
        .filter(|e| !e.is_pet && !e.is_healer())
        .filter(|e| !focus.contains(&e.entity) && !teammate_targets(ctx, entity, e.entity))
        .filter(|e| dip_target_eligible(ctx, my_team, e.entity))
        .filter(|e| trap_would_stick(ctx, entity, e.entity))
        .min_by(|a, b| {
            (a.target != Some(entity))
                .cmp(&(b.target != Some(entity)))
                .then(
                    a.position
                        .distance(my_pos)
                        .total_cmp(&b.position.distance(my_pos)),
                )
        })?
        .entity;
    Some((victim, drained))
}

/// The extra seconds of enemy melee contact a walk from `my_pos` toward
/// `goal`, until within `reach` of it, costs the Hunter over kiting away — the
/// user's weighing (AS-498): the damage the kite would have prevented that the
/// walk does not.
///
/// Every enemy melee or pet targeting the Hunter and free to move is a
/// chaser. Kiting at equal speed holds the gap a chaser has, so the kite
/// prevents a chaser's damage exactly while it is not already striking. The
/// walk is sampled: the Hunter moves straight at `goal` at `speed`, each
/// chaser straight at the Hunter at its own speed (the Hunter's base speed,
/// less its slows). A chaser the kite would have kept off costs the seconds
/// it strikes during the walk, plus [`TRAP_WALK_STICK_SECS`] when it is still
/// striking as the walk ends. A caster's damage costs nothing: the kite does
/// not prevent it either. `None` when the walk is longer than `budget`.
pub fn walk_exposure(
    ctx: &CombatContext,
    entity: Entity,
    my_pos: Vec3,
    speed: f32,
    goal: Vec3,
    reach: f32,
    budget: f32,
) -> Option<f32> {
    let me = ctx.combatants.get(&entity)?;
    let walk_len = (flat(goal).distance(flat(my_pos)) - reach).max(0.0);
    let walk = if speed > 0.0 {
        walk_len / speed
    } else {
        f32::INFINITY
    };
    if walk > budget {
        return None;
    }
    let heading = (flat(goal) - flat(my_pos)).normalize_or_zero();
    let base = speed;
    let mut extra = 0.0;
    for chaser in ctx.combatants.values().filter(|c| {
        c.team != me.team
            && c.is_alive
            && (c.is_pet || c.class.is_melee())
            && c.target == Some(entity)
            && !ctx.is_ccd(c.entity)
    }) {
        let start = flat(chaser.position);
        if start.distance(flat(my_pos)) <= TRAP_WALK_CONTACT {
            // Already striking: the kite does not prevent it either.
            continue;
        }
        let chaser_speed = base * ctx.movement_slow_multiplier(chaser.entity);
        let steps = (walk / TRAP_WALK_STEP).ceil() as usize;
        let mut at = start;
        let mut struck = 0.0;
        let mut striking = false;
        for i in 1..=steps {
            let t = (i as f32 * TRAP_WALK_STEP).min(walk);
            let hunter = flat(my_pos) + heading * speed * t;
            let to = hunter - at;
            let step = chaser_speed * TRAP_WALK_STEP;
            at += to.normalize_or_zero() * step.min(to.length());
            striking = at.distance(hunter) <= TRAP_WALK_CONTACT;
            if striking {
                struck += TRAP_WALK_STEP;
            }
        }
        extra += struck + if striking { TRAP_WALK_STICK_SECS } else { 0.0 };
    }
    Some(extra)
}

/// The trap walk (AS-498 prototype): the dip, on the user's damage weighing.
///
/// While Freezing Trap is ready and there is a [`trap_play_victim`]:
/// - its landing is clean from here ([`pressure_trap_landing`]): throw it now
///   ([`HunterDipPlan::DipCast`] with the hold);
/// - otherwise walk at it, for as long as the walk to throw range
///   ([`trap_setup_range`]) costs at most `trap_walk_worth` seconds of extra
///   melee contact over kiting ([`walk_exposure`]) and fits what is left of
///   `dip_budget`.
///
/// A walk that stops qualifying ends (traced `DipAbort`); one that starts is
/// traced `DipEnter`.
#[allow(clippy::too_many_arguments)]
fn evaluate_trap_walk(
    commands: &mut Commands,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    state: &mut KitePosture,
    directive: Option<&MovementDirective>,
    config: &DpsMovementConfig,
    now: f32,
    decision_trace: &mut DecisionTrace,
) -> HunterDipPlan {
    let live = state.dipping(now);
    let def = abilities.get_unchecked(&AbilityType::FreezingTrap);
    let ready = pre_cast_ok(
        AbilityType::FreezingTrap,
        def,
        combatant,
        my_pos,
        auras,
        None,
        ctx,
        PreCastOpts::default(),
    );
    let victim = ready
        .then(|| {
            trap_play_victim(
                ctx,
                entity,
                combatant.team,
                my_pos,
                combatant
                    .target
                    .filter(|_| config.trap_walk_spare_own_target),
                config.trap_walk_needs_teammate,
            )
        })
        .flatten();
    let mut end = |state: &mut KitePosture, decision_trace: &mut DecisionTrace| {
        if live {
            let target = state.dip_target.expect("live");
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
        }
        HunterDipPlan::Rotation
    };
    let Some((victim, instead)) = victim else {
        return end(state, decision_trace);
    };
    if pressure_trap_landing(ctx, abilities, combatant.team, my_pos, victim).is_ok() {
        let mut completed = *state;
        completed.dip_target = None;
        completed.dip_until = 0.0;
        return HunterDipPlan::DipCast {
            target: victim,
            completed_state: completed,
            hold: Some(instead),
        };
    }
    let Some(goal) = ctx.combatants.get(&victim).map(|v| v.position) else {
        return end(state, decision_trace);
    };
    let budget = if live {
        state.dip_until - now
    } else {
        config.dip_budget
    };
    let speed = combatant.base_movement_speed * ctx.movement_slow_multiplier(entity);
    let worth = walk_exposure(
        ctx,
        entity,
        my_pos,
        speed,
        goal,
        trap_setup_range(abilities),
        budget,
    )
    .is_some_and(|extra| extra <= config.trap_walk_worth);
    if !worth {
        return end(state, decision_trace);
    }
    if !live || state.dip_target != Some(victim) {
        state.dip_target = Some(victim);
        if !live {
            state.dip_until = now + config.dip_budget;
        }
        emit_dip(
            decision_trace,
            ctx,
            victim,
            my_pos,
            MovementTrigger::DipEnter,
        );
    }
    commands.entity(entity).try_insert(MovementDirective {
        goal: MovementGoal::Entity(victim),
        expires: state.dip_until,
        committed_until: state.dip_until,
    });
    HunterDipPlan::Walking
}

/// Where to aim a Freezing Trap so it lands on — or just ahead of — `target`
/// when it arms. A target going nowhere (casting, rooted or held:
/// [`forecast_velocity`] is zero) is aimed at directly; a moving target is led
/// along its forecast velocity by the trap's travel + arm time so the trap
/// drops into its path, at the speed its slows leave it. The lead is capped so
/// a long arm window can't fling the aim across the arena when the target
/// turns.
pub fn trap_lead_landing(ctx: &CombatContext, target: Entity, my_pos: Vec3) -> Option<Vec3> {
    let info = ctx.combatants.get(&target)?;
    let travel = my_pos.distance(info.position) / TRAP_LAUNCH_SPEED;
    let lead_time = TRAP_ARM_DELAY + travel;
    let lead =
        (forecast_velocity(ctx, target) * lead_time).clamp_length_max(TRAP_TRIGGER_RADIUS * 3.0);
    Some(info.position + lead)
}

/// How `e` is forecast to move over the next few seconds, for every trap
/// prediction the Hunter makes: along its heading
/// ([`CombatantInfo::velocity`](super::CombatantInfo::velocity), zero while it
/// casts) at the speed its slows leave it
/// ([`effective_movement_speed`](crate::states::play_match::combat_core::effective_movement_speed),
/// the one derivation movement uses), and not at all while it is rooted or
/// held in hard crowd control ([`CombatContext::is_ccd`], which counts both).
pub fn forecast_velocity(ctx: &CombatContext, e: Entity) -> Vec3 {
    let Some(info) = ctx.combatants.get(&e) else {
        return Vec3::ZERO;
    };
    // `is_ccd` counts a root with the holds: rooted, stunned, feared,
    // polymorphed, incapacitated or cycloned, the unit goes nowhere.
    if ctx.is_ccd(e) {
        return Vec3::ZERO;
    }
    let auras = ActiveAuras {
        auras: ctx.active_auras.get(&e).cloned().unwrap_or_default(),
    };
    // The share of its base speed its auras leave it: exactly 1.0 with no
    // slow, so an unslowed heading is carried through unchanged.
    let share = crate::states::play_match::combat_core::effective_movement_speed(1.0, Some(&auras));
    info.velocity * share
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
