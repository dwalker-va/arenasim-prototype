//! Druid AI Module
//!
//! The Druid is a PROACTIVE heal-over-time healer. The other healers heal on
//! thresholds — wait for an ally to drop, then cast. Every heal the Druid has
//! is an instant, so its AI instead keeps heals rolling on the ally the enemy
//! is attacking BEFORE the damage lands, and paces its mana so it is still
//! healing when arena dampening arrives.
//!
//! The rotation, in priority order:
//! 1. Mark of the Wild on any ally without it (pre-match, in practice) — but
//!    not while a teammate is dying: then it waits behind the heals (7).
//! 2. Swiftmend — the emergency button — on a dying ally carrying Rejuvenation.
//! 3. An emergency Rejuvenation on a dying ally without one, which arms
//!    Swiftmend for the next global cooldown.
//! 4. Innervate on itself when its mana runs low.
//! 5. Rejuvenation kept on the FOCUSED ally — the one the enemy is attacking,
//!    or the one an enemy is closing on (the pre-HoT, before damage lands).
//! 6. Lifebloom kept rolling on the focused ally while it is UNDER ATTACK, its
//!    bloom managed on purpose (see [`lifebloom_decision`]). A threat that is
//!    only closing gets the Rejuvenation, not the stack: rolling three stacks
//!    on an ally nobody is hitting yet is mana spent on overheal.
//! 7. What a teammate below `urgency_hp_threshold` is owed, and the peels (see
//!    [`DruidTurn::try_dying_heal`]): the teammate's Rejuvenation; Cyclone on
//!    an enemy attacking the focus once it is below [`DRUID_URGENT_HP`]; Roots
//!    on a melee enemy or pet attacking or closing on a dying teammate; the
//!    teammate's
//!    Lifebloom stack while it is under attack — then a Mark of the Wild step 1
//!    held back.
//! 8. Control, when the kill needs its healer gone or a melee enemy or pet is
//!    on the Druid or its focus (see [`DruidTurn::try_control`]): Cyclone, then
//!    Entangling Roots.
//! 9. Rejuvenation on any other injured ally.
//! 10. Moonfire on the kill target.
//!
//! **A dying teammate is healed before any damage or utility.** A teammate
//! below `urgency_hp_threshold` that the Druid can reach (range, then sight)
//! gets what its kit can still add — below [`DRUID_EMERGENCY_HP`], Swiftmend
//! or the Rejuvenation that arms it (2-3); otherwise its own Rejuvenation, and
//! a Lifebloom stack while it is under attack (7) — and a melee enemy or pet
//! attacking or closing on it is rooted (the Roots PEEL, 7), before Mark of the Wild, a
//! Cyclone or Entangling Roots that is not a peel, or Moonfire. And while the
//! medic walk is live and its teammate is NOT yet in reach, those damage and
//! utility casts are held outright: every Druid spell is an instant on the
//! global cooldown, so a Moonfire cast while the walk rounds a pillar edge
//! spends the 1.5s in which sight returns, and the heal waits for it. The
//! peels (the Cyclone for a focus below [`DRUID_URGENT_HP`], the Roots for a
//! dying teammate), Travel Form, Innervate and the heals are never held.
//!
//! Between the emergency steps and Innervate sits the ESCAPE SHIFT: Travel
//! Form when a threat is on the Druid and it is rooted or slowed, or when a
//! melee is beating on it (see [`shift_trigger`]). The shift breaks the
//! impairment and the posture machine runs; while shifted the Druid casts
//! nothing. It shifts back — free, no global cooldown — once it has been
//! shifted [`DRUID_MIN_FORM_SECS`], no chaser is within striking reach (nor
//! within the danger radius, for the first [`DRUID_MAX_CHASE_SECS`]) and it
//! has work (an ally to heal or an enemy in spell reach), or at once if it
//! is rooted again ([`should_leave_form`]).
//!
//! Steps 5-10 answer to the MANA GOVERNOR ([`mana_reserve`]): proactive heals,
//! control and damage are paid for only out of mana above a reserve that
//! shrinks as the match heads into dampening. The emergency steps (2, 3), the
//! dying teammate's heals (7), the shift and a peel for a dying ally never ask
//! it, and neither does a focused
//! ally who has dropped below [`DRUID_URGENT_HP`].
//!
//! Movement is the shared caster-healer posture machine
//! (`caster_healer_posture`) on the `druid:` block of `movement.ron`.
#![allow(clippy::too_many_arguments)]

use std::collections::HashSet;

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_config::AbilityDefinitions;
use crate::states::play_match::combat_core::calculate_cast_time;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::*;
use crate::states::play_match::decision_trace::{
    DecisionEventBuilder, DecisionTrace, RejectionReason,
};
use crate::states::play_match::movement_config::MovementConfig;

use super::super::utils::log_ability_use;
use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, unreached_reason, PreCastOpts};
use super::{CombatContext, CombatantInfo};

/// An ally below this HP fraction is dying: Swiftmend, or the Rejuvenation that
/// arms it, before anything else.
pub const DRUID_EMERGENCY_HP: f32 = 0.45;
/// An ally below this HP fraction who is not the focus still gets a
/// Rejuvenation (step 7).
pub const DRUID_TOP_UP_HP: f32 = 0.8;
/// A focused ally below this HP fraction gets its heals regardless of the mana
/// governor — they are no longer proactive.
pub const DRUID_URGENT_HP: f32 = 0.6;
/// Self-Innervate once mana falls below this fraction of the pool.
pub const DRUID_INNERVATE_MANA_PCT: f32 = 0.35;
/// An enemy targeting an ally from within its own preferred range plus this
/// slack is ATTACKING it; from farther out, but inside
/// `shared.threat_intent_radius`, it is CLOSING on it.
pub const DRUID_ATTACK_RANGE_SLACK: f32 = 5.0;
/// Recast a Rejuvenation with less than this left on it — about half a tick —
/// so it does not lapse between ticks.
pub const REJUVENATION_REFRESH_SECS: f32 = 1.5;
/// A full Lifebloom stack with less than this left is at its decision point:
/// refresh it and keep it rolling, or let it bloom.
pub const LIFEBLOOM_DECISION_SECS: f32 = 1.5;
/// At the decision point, a focused ally below this HP fraction wants the
/// bloom's burst heal more than the rolling ticks: the Druid lets it bloom.
pub const LIFEBLOOM_BLOOM_HP: f32 = 0.75;
/// The mana governor's reserve at the gates, as a fraction of the pool. It
/// shrinks linearly to nothing by [`GOVERNOR_HORIZON_SECS`].
pub const GOVERNOR_RESERVE_AT_GATES: f32 = 0.5;
/// When the governor stops holding mana back: the moment arena dampening
/// reaches 50% (it starts at `DAMPENING_START_SECS` and ramps to 100% over
/// `DAMPENING_RAMP_SECS`). Mana saved past it buys heals worth half as much.
pub const GOVERNOR_HORIZON_SECS: f32 = DAMPENING_START_SECS + DAMPENING_RAMP_SECS * 0.5;
/// A melee attacking the Druid while it is below this HP fraction is reason
/// enough to shift and open distance, rooted or not.
pub const DRUID_SHIFT_OPEN_HP: f32 = 0.6;
/// Cyclone the enemy healer once the kill target is below this HP fraction:
/// six seconds of no heals is worth the most when the kill is close.
pub const DRUID_CYCLONE_KILL_HP: f32 = 0.4;

/// Why the Druid shifts into Travel Form, when it should.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftReason {
    /// Rooted, or snared with a melee or pet chasing it: the shift breaks the
    /// impairment.
    BreakImpairment,
    /// A melee is beating on a hurt Druid: the shift opens distance.
    OpenDistance,
}

/// What the escape-shift rule reads, for [`shift_trigger`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShiftView {
    /// The posture machine's PRESSURED trigger: a visible threat is on the
    /// Druid or closing on it.
    pub pressured: bool,
    pub rooted: bool,
    pub snared: bool,
    /// A visible melee enemy or pet targeting the Druid within the intent
    /// radius — the kind of threat a faster Druid can leave behind.
    pub chaser: bool,
    /// A visible melee enemy or pet targeting the Druid within striking reach.
    pub melee_on_me: bool,
    pub self_health_pct: f32,
}

impl ShiftView {
    /// The view of the Druid at `my_pos` with `auras` on it.
    pub fn of(
        ctx: &CombatContext,
        entity: Entity,
        my_pos: Vec3,
        auras: Option<&ActiveAuras>,
        pressured: bool,
        threat_radius: f32,
        self_health_pct: f32,
    ) -> Self {
        let has =
            |ty: AuraType| auras.is_some_and(|a| a.auras.iter().any(|aura| aura.effect_type == ty));
        let chasers = || {
            ctx.enemies_targeting(entity)
                .into_iter()
                .filter(|e| e.class.is_melee() || e.is_pet)
        };
        Self {
            pressured,
            rooted: has(AuraType::Root),
            snared: has(AuraType::MovementSpeedSlow),
            chaser: chasers().any(|e| e.position.distance(my_pos) <= threat_radius),
            melee_on_me: chasers()
                .any(|e| e.position.distance(my_pos) <= MELEE_RANGE + DRUID_ATTACK_RANGE_SLACK),
            self_health_pct,
        }
    }
}

/// The escape-shift rule, pure so it can be tested on its own. Only while
/// PRESSURED:
/// - rooted → break it;
/// - snared WITH a melee or pet chasing it → break it. A snare with no chaser
///   is shrugged off: the form outruns legs, not spells, so shifting out of a
///   Frostbolt or Frost Shock from a caster buys nothing;
/// - a melee attacking it while it is below [`DRUID_SHIFT_OPEN_HP`] → open
///   distance.
pub fn shift_trigger(view: ShiftView) -> Option<ShiftReason> {
    if !view.pressured {
        return None;
    }
    if view.rooted || (view.snared && view.chaser) {
        return Some(ShiftReason::BreakImpairment);
    }
    if view.melee_on_me && view.self_health_pct < DRUID_SHIFT_OPEN_HP {
        return Some(ShiftReason::OpenDistance);
    }
    None
}

/// The least time a Druid spends in Travel Form before it may leave to cast:
/// long enough to open real distance at the form's speed, and two global
/// cooldowns so a shift is never paid for and thrown straight back.
pub const DRUID_MIN_FORM_SECS: f32 = 3.0;
/// How long a chaser that cannot close to striking reach may keep the Druid
/// shifted. Past this the Druid stops running and uses its kit: its heals are
/// instants, and a chaser that never lands a hit is one it can outpace again
/// on the next shift.
pub const DRUID_MAX_CHASE_SECS: f32 = 8.0;
/// How long a Druid that left the form of its own accord holds the shift back:
/// one global cooldown, as the cast it left to make would. A chaser that
/// closes in that window is not answered with a shift straight back.
pub const DRUID_RESHIFT_HOLD_SECS: f32 = GCD;

/// What a shifted Druid can see from inside the form, for [`should_leave_form`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormView {
    /// Seconds since it shifted.
    pub in_form_secs: f32,
    /// Rooted again while shifted.
    pub rooted: bool,
    /// A visible melee enemy or pet within the danger radius — a chaser the
    /// form has not yet shaken off.
    pub chaser_near: bool,
    /// A visible melee enemy or pet within striking reach of the Druid.
    pub chaser_striking: bool,
    /// An ally in heal range below [`DRUID_TOP_UP_HP`].
    pub healing_needed: bool,
    /// A visible enemy within the Druid's spell reach (Moonfire's range):
    /// something to Moonfire, root or cyclone.
    pub enemy_in_reach: bool,
    /// [`shift_trigger`] would fire for the Druid as it stands: a leave now
    /// would be answered by a shift straight back.
    pub would_reshift: bool,
}

/// Whether a shifted Druid leaves Travel Form. Leaving is free and costs no
/// global cooldown.
/// - **Rooted again**: it leaves at once, whatever else holds — a rooted form
///   is no faster than no form, and the rotation re-shifts to break the root
///   if a threat is still on it. A new SNARE does not: the form still outruns
///   a slowed chaser, and shifting out and back in under a Mage's Frostbolts
///   would spend mana and a global cooldown on every bolt.
/// - **Never into a re-shift**: not with a chaser in striking reach, and not
///   while [`shift_trigger`] would fire — leaving would buy one frame out of
///   form for 25 mana and a global cooldown on the shift straight back. And a
///   Druid that leaves of its own accord holds the shift back for
///   [`DRUID_RESHIFT_HOLD_SECS`], so a chaser closing just after is not
///   answered with one either.
/// - Otherwise it leaves once it is **safe** — it has been shifted at least
///   [`DRUID_MIN_FORM_SECS`], and no chaser is within the danger radius
///   unless it has run for [`DRUID_MAX_CHASE_SECS`] without shaking it —
///   **and has work**: an ally to heal, or an enemy in reach of its spells.
///
/// Safe with nothing in reach, it stays shifted: the form is faster and
/// there is nothing to cast. A caster hitting the Druid from range does not
/// keep it shifted — the form buys nothing against a spell.
pub fn should_leave_form(view: FormView) -> bool {
    if view.rooted {
        return true;
    }
    if view.chaser_striking || view.would_reshift {
        return false;
    }
    view.in_form_secs >= DRUID_MIN_FORM_SECS
        && (!view.chaser_near || view.in_form_secs >= DRUID_MAX_CHASE_SECS)
        && (view.healing_needed || view.enemy_in_reach)
}

/// The mana the governor holds back `time_since_gates` seconds into the fight:
/// [`GOVERNOR_RESERVE_AT_GATES`] of the pool at the gates, falling linearly to
/// zero at [`GOVERNOR_HORIZON_SECS`]. Proactive spending must leave at least
/// this much behind.
pub fn mana_reserve(max_mana: f32, time_since_gates: f32) -> f32 {
    let remaining = (1.0 - time_since_gates / GOVERNOR_HORIZON_SECS).clamp(0.0, 1.0);
    max_mana * GOVERNOR_RESERVE_AT_GATES * remaining
}

/// Whether the governor lets a proactive cast of `cost` through.
fn governor_allows(combatant: &Combatant, cost: f32, time_since_gates: f32) -> bool {
    combatant.current_mana - cost >= mana_reserve(combatant.max_mana, time_since_gates)
}

/// What the Druid does with Lifebloom on its focused ally this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifebloomDecision {
    /// Cast it: the first stack, another stack, or a refresh that keeps a full
    /// stack rolling.
    Cast,
    /// A full stack with time left: nothing to do.
    Hold,
    /// A full stack at its decision point on an ally who needs the burst:
    /// leave it to bloom.
    LetBloom,
}

/// The Lifebloom rule, pure so it can be tested on its own:
/// - an ally nobody is attacking yet (`under_attack` false): hold — the
///   Rejuvenation is the pre-HoT, and a stack rolled on an untouched ally is
///   overheal;
/// - no Lifebloom, or fewer than its max stacks: cast (build the stack);
/// - a full stack with more than [`LIFEBLOOM_DECISION_SECS`] left: hold;
/// - a full stack at its decision point: refresh it to keep it rolling while
///   the ally is healthy, and let it bloom when the ally is below
///   [`LIFEBLOOM_BLOOM_HP`] — a refresh would throw the bloom away exactly
///   when its burst is worth most.
pub fn lifebloom_decision(
    existing: Option<&Aura>,
    ally_health_pct: f32,
    under_attack: bool,
) -> LifebloomDecision {
    if !under_attack {
        return LifebloomDecision::Hold;
    }
    let Some(aura) = existing else {
        return LifebloomDecision::Cast;
    };
    let full = aura.stacks.is_none_or(|s| s.count >= s.max);
    if !full {
        return LifebloomDecision::Cast;
    }
    if aura.duration > LIFEBLOOM_DECISION_SECS {
        return LifebloomDecision::Hold;
    }
    if ally_health_pct < LIFEBLOOM_BLOOM_HP {
        LifebloomDecision::LetBloom
    } else {
        LifebloomDecision::Cast
    }
}

/// The threat on one ally: how many visible enemies are ATTACKING it —
/// targeting it from within their own reach — and how many are CLOSING on it
/// (targeting it from farther out, inside `threat_radius`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AllyThreat {
    pub attacking: usize,
    pub closing: usize,
}

impl AllyThreat {
    pub fn on(ctx: &CombatContext, ally: &CombatantInfo, threat_radius: f32) -> Self {
        let mut threat = AllyThreat {
            attacking: 0,
            closing: 0,
        };
        for enemy in ctx
            .combatants
            .values()
            .filter(|e| e.team != ally.team && e.is_alive && e.target == Some(ally.entity))
        {
            let reach = match enemy.pet_type {
                Some(pet) => pet.preferred_range(),
                None => enemy.class.preferred_range(),
            } + DRUID_ATTACK_RANGE_SLACK;
            let distance = enemy.position.distance(ally.position);
            if distance <= reach {
                threat.attacking += 1;
            } else if distance <= threat_radius {
                threat.closing += 1;
            }
        }
        threat
    }

    fn any(self) -> bool {
        self.attacking + self.closing > 0
    }
}

/// The ally the Druid keeps its heals rolling on: the one the most visible
/// enemies are attacking; failing that, the one the most are closing on (the
/// pre-HoT, before any damage lands). Ties go to the lower HP fraction, then
/// to entity order. `None` when nobody is under threat. Only allies `eligible`
/// accepts are ranked: the heal focus passes the allies its HoTs are not kept
/// off by sight alone ([`super::sight_blocks`]), the control focus every ally.
pub fn focused_ally<'c>(
    ctx: &'c CombatContext,
    threat_radius: f32,
    eligible: impl Fn(&CombatantInfo) -> bool,
) -> Option<(&'c CombatantInfo, AllyThreat)> {
    ctx.alive_allies()
        .into_iter()
        // A cycloned ally takes no heal (`CombatContext::is_cycloned`).
        .filter(|ally| !ctx.is_cycloned(ally.entity))
        .filter(|ally| eligible(ally))
        .map(|ally| (ally, AllyThreat::on(ctx, ally, threat_radius)))
        .filter(|(_, threat)| threat.any())
        .max_by(|(a, a_threat), (b, b_threat)| {
            a_threat
                .cmp(b_threat)
                // Lower HP wins, so compare reversed.
                .then(
                    b.health_pct()
                        .partial_cmp(&a.health_pct())
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
                // Lower entity wins the last tie, again reversed.
                .then(b.entity.cmp(&a.entity))
        })
}

/// This Druid's aura from `ability` on `target`, if any — the one it cast.
/// Another Druid's Lifebloom on the same ally is that Druid's stack, not this
/// one's ([`StackScope::PerCaster`]).
fn own_aura<'c>(
    ctx: &'c CombatContext,
    abilities: &AbilityDefinitions,
    caster: Entity,
    target: Entity,
    ability: AbilityType,
) -> Option<&'c Aura> {
    let name = &abilities.get_unchecked(&ability).name;
    ctx.active_auras.get(&target).and_then(|auras| {
        auras
            .iter()
            .find(|a| &a.ability_name == name && a.caster == Some(caster))
    })
}

/// Druid AI: decides and executes one ability per global cooldown.
///
/// `medic_walk` is the teammate a live medic walk is walking to (the posture
/// plan's): see the dying-teammate rule in the module docs.
/// `time_since_gates` feeds the mana governor. `gates_opened` holds every
/// combat action until the gates open; only Mark of the Wild is cast before.
/// `marked_this_frame` holds the allies a Druid has already sent a Mark of the
/// Wild to this frame, shared by every Druid's turn — see
/// [`DruidTurn::try_mark_of_the_wild`].
/// Returns `true` if an action was taken this frame.
pub fn decide_druid_action(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityDefinitions,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    movement: &MovementConfig,
    pressured: bool,
    medic_walk: Option<Entity>,
    gates_opened: bool,
    time_since_gates: f32,
    marked_this_frame: &mut HashSet<Entity>,
    decision_trace: &mut DecisionTrace,
) -> bool {
    // Shifted: nothing is cast in Travel Form, so the only decision is
    // whether to leave it. Leaving is free and on no global cooldown, so it
    // is decided before the GCD gate. It is not an ability decision — the
    // posture machine traces the ESCAPE it ends, and the log says it.
    if let Some(form) = auras.and_then(|a| {
        a.auras
            .iter()
            .find(|aura| aura.effect_type == AuraType::TravelForm)
    }) {
        let form_duration = abilities
            .get_unchecked(&AbilityType::TravelForm)
            .applies_aura
            .as_ref()
            .map_or(form.duration, |a| a.duration);
        let view = FormView {
            in_form_secs: form_duration - form.duration,
            rooted: auras.is_some_and(|a| {
                a.auras
                    .iter()
                    .any(|aura| aura.effect_type == AuraType::Root)
            }),
            chaser_near: ctx
                .visible_enemies_within(entity, my_pos, movement.shared.danger_radius)
                .iter()
                .any(|e| e.class.is_melee() || e.is_pet),
            chaser_striking: ctx
                .visible_enemies_within(entity, my_pos, MELEE_RANGE + DRUID_ATTACK_RANGE_SLACK)
                .iter()
                .any(|e| e.class.is_melee() || e.is_pet),
            healing_needed: ctx.alive_allies().into_iter().any(|a| {
                a.health_pct() < DRUID_TOP_UP_HP
                    && my_pos.distance(a.position) <= movement.shared.heal_range
            }),
            enemy_in_reach: !ctx
                .visible_enemies_within(
                    entity,
                    my_pos,
                    abilities.get_unchecked(&AbilityType::Moonfire).range,
                )
                .is_empty(),
            would_reshift: shift_trigger(ShiftView::of(
                ctx,
                entity,
                my_pos,
                auras,
                pressured,
                movement.shared.threat_intent_radius,
                combatant.current_health / combatant.max_health,
            ))
            .is_some(),
        };
        if gates_opened && should_leave_form(view) {
            commands.spawn(ShapeshiftPending {
                caster: entity,
                shift: Shift::Out,
            });
            // Left to cast, so it holds the shift back as a cast's global
            // cooldown would. A rooted exit holds nothing back: it leaves
            // to re-shift through the root.
            if !view.rooted {
                combatant
                    .ability_cooldowns
                    .insert(AbilityType::TravelForm, DRUID_RESHIFT_HOLD_SECS);
            }
            return true;
        }
        return false;
    }

    if combatant.global_cooldown > 0.0 {
        return false;
    }

    let Some(mut builder) = ctx.start_ability_decision(decision_trace, combatant.target, my_pos)
    else {
        return false;
    };

    let acted = {
        let mut turn = DruidTurn {
            commands,
            combat_log,
            abilities,
            entity,
            my_pos,
            auras,
            ctx,
            heal_range: movement.shared.heal_range,
            threat_radius: movement.shared.threat_intent_radius,
            urgency_hp: movement.shared.urgency_hp_threshold,
            medic_walk,
            pressured,
            time_since_gates,
            marked_this_frame,
            builder: &mut builder,
        };
        turn.take(combatant, gates_opened)
    };
    builder.finish();
    acted
}

/// How [`DruidTurn::melee_threat`] treats a candidate beyond Roots' range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootsPick {
    /// The Roots PEEL: reach-first, range then sight, as `ally_reach` is — a
    /// candidate beyond range is passed over for one Roots can land on.
    Peel,
    /// Step 8's Roots: a candidate beyond range is still picked and refused,
    /// the rule for every chooser that ranks without a range filter.
    Step8,
}

/// The candidates [`DruidTurn::melee_threat`] passed over, for the rejection
/// a pick that found none reports.
#[derive(Debug, Clone, Copy, Default)]
struct RootsSkipped {
    /// One immune to damage.
    immune: bool,
    /// One in range but out of sight.
    occluded: bool,
}

/// What the Cyclone peel ([`DruidTurn::try_cyclone_peel`]) did this turn.
#[derive(Debug, Clone, Copy, Default)]
struct CyclonePeel {
    /// It cast.
    cast: bool,
    /// It found an attacker in sight to cyclone.
    picked: bool,
    /// It passed over an attacker it could reach but for sight.
    occluded: bool,
}

/// One decision's worth of context, so the rotation's steps share it instead
/// of each taking a dozen parameters.
struct DruidTurn<'a, 'b, 'w, 's, 'c> {
    commands: &'a mut Commands<'w, 's>,
    combat_log: &'a mut CombatLog,
    abilities: &'a AbilityDefinitions,
    entity: Entity,
    my_pos: Vec3,
    auras: Option<&'a ActiveAuras>,
    ctx: &'a CombatContext<'c>,
    heal_range: f32,
    threat_radius: f32,
    /// `shared.urgency_hp_threshold`: a teammate below it is dying.
    urgency_hp: f32,
    /// The teammate a live medic walk is walking to.
    medic_walk: Option<Entity>,
    /// The posture machine's PRESSURED trigger this tick.
    pressured: bool,
    time_since_gates: f32,
    /// Allies sent a Mark of the Wild this frame, by any Druid.
    marked_this_frame: &'a mut HashSet<Entity>,
    builder: &'a mut DecisionEventBuilder<'b>,
}

impl DruidTurn<'_, '_, '_, '_, '_> {
    /// The whole turn: the pre-match buff (step 1), then, once the gates are
    /// open, the rotation. A dying teammate moves Mark of the Wild behind its
    /// heals (step 7).
    fn take(&mut self, combatant: &mut Combatant, gates_opened: bool) -> bool {
        let dying = gates_opened && self.teammate_dying();
        (!dying && self.try_mark_of_the_wild(combatant, false))
            || (gates_opened && self.rotation(combatant, dying))
    }

    /// Steps 2-10 — everything after the pre-match buff. Every heal is an
    /// instant, so an ESCAPE window has nothing to defer: the Druid heals on
    /// the run.
    ///
    /// `mark_waits`: a teammate is dying ([`Self::teammate_dying`]), so Mark
    /// of the Wild was held back from step 1 for step 7.
    fn rotation(&mut self, combatant: &mut Combatant, mark_waits: bool) -> bool {
        // 2-3. The emergency button, and the Rejuvenation that arms it.
        // Reach (range, then sight) filters before health ranks: an occluded
        // dying ally yields to the lowest dying one in sight.
        let dying = self
            .ctx
            .lowest_health_ally_in_reach(DRUID_EMERGENCY_HP, self.heal_range, self.my_pos)
            .map(|a| (a.entity, a.position));
        if let Ok((ally, ally_pos)) = dying {
            if self.has_own(ally, AbilityType::Rejuvenation) {
                if self.cast(combatant, AbilityType::Swiftmend, ally, ally_pos) {
                    return true;
                }
            } else {
                self.builder.reject(
                    AbilityType::Swiftmend,
                    RejectionReason::PreconditionUnmet {
                        note: "dying ally carries no Rejuvenation to consume".to_string(),
                    },
                );
                if self.cast(combatant, AbilityType::Rejuvenation, ally, ally_pos) {
                    return true;
                }
            }
        } else if let Err(reason) = dying {
            let reason = self.unreached(combatant, AbilityType::Swiftmend, reason);
            self.builder.reject(AbilityType::Swiftmend, reason);
        }

        // The escape shift.
        if self.try_travel_form(combatant) {
            return true;
        }

        // 4. Self-Innervate when the pool runs low.
        if combatant.current_mana < combatant.max_mana * DRUID_INNERVATE_MANA_PCT {
            if self.cast(combatant, AbilityType::Innervate, self.entity, self.my_pos) {
                return true;
            }
        } else {
            self.builder.reject(
                AbilityType::Innervate,
                RejectionReason::PreconditionUnmet {
                    note: "mana above the Innervate threshold".to_string(),
                },
            );
        }

        // 5-6. Keep the focused ally's heals rolling. An ally the HoTs could
        // reach but for sight is not ranked, so an occluded focus yields to the
        // most-threatened ally in sight; a focus beyond heal range is still
        // passed over, as it always was.
        let (ctx, my_pos, heal_range) = (self.ctx, self.my_pos, self.heal_range);
        let focus = focused_ally(ctx, self.threat_radius, |a| {
            !super::sight_blocks(ctx, heal_range, my_pos, a.position)
        })
        .filter(|(a, _)| my_pos.distance(a.position) <= heal_range)
        .map(|(a, threat)| (a.entity, a.position, a.health_pct(), threat));
        if let Some((ally, ally_pos, hp, threat)) = focus {
            let urgent = hp < DRUID_URGENT_HP;

            let rejuv_remaining = self
                .own(ally, AbilityType::Rejuvenation)
                .map(|a| a.duration);
            if rejuv_remaining.is_none_or(|left| left < REJUVENATION_REFRESH_SECS) {
                if self.governed_cast(combatant, AbilityType::Rejuvenation, ally, ally_pos, urgent)
                {
                    return true;
                }
            } else {
                self.builder
                    .reject(AbilityType::Rejuvenation, RejectionReason::AlreadyApplied);
            }

            let lifebloom = self.own(ally, AbilityType::Lifebloom).cloned();
            match lifebloom_decision(lifebloom.as_ref(), hp, threat.attacking > 0) {
                LifebloomDecision::Cast => {
                    if self.governed_cast(combatant, AbilityType::Lifebloom, ally, ally_pos, urgent)
                    {
                        return true;
                    }
                }
                LifebloomDecision::Hold if threat.attacking == 0 => {
                    self.builder.reject(
                        AbilityType::Lifebloom,
                        RejectionReason::PreconditionUnmet {
                            note: "focus is only threatened, not attacked: Rejuvenation is \
                                   the pre-HoT"
                                .to_string(),
                        },
                    );
                }
                LifebloomDecision::Hold => {
                    self.builder
                        .reject(AbilityType::Lifebloom, RejectionReason::AlreadyApplied);
                }
                LifebloomDecision::LetBloom => {
                    self.builder.reject(
                        AbilityType::Lifebloom,
                        RejectionReason::PreconditionUnmet {
                            note: "letting a full stack bloom on a hurt ally".to_string(),
                        },
                    );
                }
            }
        } else {
            self.builder
                .reject(AbilityType::Lifebloom, RejectionReason::NoValidTarget);
        }

        // The control focus: the ally the peels guard. It is ranked over every
        // ally, sight or not: its attackers may be in sight when it is not.
        let control_focus = focused_ally(ctx, self.threat_radius, |_| true)
            .filter(|(a, _)| my_pos.distance(a.position) <= heal_range)
            .map(|(a, _)| (a.entity, a.health_pct()));

        // 7. A dying teammate's heals and the peels, then the Mark step 1 held
        // back — unless the medic walk's teammate is still out of reach, when
        // the Mark waits with the rest of the damage and utility.
        let held = self.held_for_medic_walk();
        let mut cyclone_peel = CyclonePeel::default();
        if self.try_dying_heal(combatant, control_focus, &mut cyclone_peel) {
            return true;
        }
        if mark_waits && self.try_mark_of_the_wild(combatant, held) {
            return true;
        }

        // 8. Control: the enemy healer, and Roots on a melee on the Druid or
        // its focus.
        if self.try_control(combatant, control_focus, held, cyclone_peel) {
            return true;
        }

        // 9. Rejuvenation on anyone else who is hurt and has none.
        let focus_entity = focus.map(|(e, _, _, _)| e);
        let top_up = self
            .ctx
            .alive_allies()
            .into_iter()
            .filter(|a| {
                a.health_pct() < DRUID_TOP_UP_HP
                    && super::cast_reach(self.ctx, self.heal_range, self.my_pos, a.position)
                        == super::CastReach::Reaches
                    && !self.ctx.is_cycloned(a.entity)
                    && Some(a.entity) != focus_entity
                    && !self.has_own(a.entity, AbilityType::Rejuvenation)
            })
            .min_by(|a, b| {
                a.health_pct()
                    .partial_cmp(&b.health_pct())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|a| (a.entity, a.position));
        if let Some((ally, ally_pos)) = top_up {
            if self.governed_cast(combatant, AbilityType::Rejuvenation, ally, ally_pos, false) {
                return true;
            }
        }

        // 10. Moonfire on the kill target, when it is not already burning.
        self.try_moonfire(combatant, held)
    }

    /// Whether a teammate (the Druid included, pets excluded) is DYING: below
    /// `urgency_hp_threshold` within heal range, in sight or not — or the
    /// medic walk is live, which is the same thing behind cover.
    fn teammate_dying(&self) -> bool {
        self.medic_walk.is_some()
            || self.ctx.alive_allies().into_iter().any(|a| {
                !a.is_pet
                    && !self.ctx.is_cycloned(a.entity)
                    && a.health_pct() < self.urgency_hp
                    && self.my_pos.distance(a.position) <= self.heal_range
            })
    }

    /// Whether damage and utility are held for the medic walk: it is live,
    /// and its teammate is not yet in reach of a heal (range, then sight).
    fn held_for_medic_walk(&self) -> bool {
        self.medic_walk.is_some_and(|ally| {
            self.ctx.combatants.get(&ally).is_some_and(|a| {
                super::cast_reach(self.ctx, self.heal_range, self.my_pos, a.position)
                    != super::CastReach::Reaches
            })
        })
    }

    /// The rejection a held damage or utility cast reports.
    fn held_reason() -> RejectionReason {
        RejectionReason::PreconditionUnmet {
            note: "holding the global cooldown for a dying teammate the medic walk has not \
                   reached"
                .to_string(),
        }
    }

    /// Step 7, for the teammates below `urgency_hp_threshold`, and the peels.
    /// Never governed and never held: they are dying.
    /// 1. The lowest one in reach (range, then sight) gets its own
    ///    Rejuvenation when it carries none, or it is in its refresh window.
    /// 2. The peels: the Cyclone for the control focus
    ///    ([`Self::try_cyclone_peel`], whose verdict lands in `cyclone_peel`
    ///    for step 8 to read), then the Roots for a dying teammate
    ///    ([`Self::try_roots_peel`]).
    /// 3. That lowest one gets a Lifebloom as [`lifebloom_decision`] rules it:
    ///    only while it is under attack — a threat that is only closing gets
    ///    the Rejuvenation and the peel, not the stack.
    ///
    /// Swiftmend and the Rejuvenation that arms it are steps 2-3, for an ally
    /// below [`DRUID_EMERGENCY_HP`]. With nothing left to add, the rotation
    /// goes on.
    fn try_dying_heal(
        &mut self,
        combatant: &mut Combatant,
        control_focus: Option<(Entity, f32)>,
        cyclone_peel: &mut CyclonePeel,
    ) -> bool {
        let lowest = self
            .ctx
            .lowest_health_ally_in_reach(self.urgency_hp, self.heal_range, self.my_pos)
            .ok()
            .map(|a| {
                (
                    a.entity,
                    a.position,
                    a.health_pct(),
                    AllyThreat::on(self.ctx, a, self.threat_radius),
                )
            });
        if let Some((ally, ally_pos, _, _)) = lowest {
            if self
                .own(ally, AbilityType::Rejuvenation)
                .is_none_or(|a| a.duration < REJUVENATION_REFRESH_SECS)
                && self.cast(combatant, AbilityType::Rejuvenation, ally, ally_pos)
            {
                return true;
            }
        }
        *cyclone_peel = self.try_cyclone_peel(combatant, control_focus);
        if cyclone_peel.cast || self.try_roots_peel(combatant) {
            return true;
        }
        let Some((ally, ally_pos, hp, threat)) = lowest else {
            return false;
        };
        let lifebloom = self.own(ally, AbilityType::Lifebloom).cloned();
        lifebloom_decision(lifebloom.as_ref(), hp, threat.attacking > 0) == LifebloomDecision::Cast
            && self.cast(combatant, AbilityType::Lifebloom, ally, ally_pos)
    }

    /// The Cyclone PEEL: the control focus is below [`DRUID_URGENT_HP`] and a
    /// visible enemy (not a pet) is attacking it — cyclone the nearest such
    /// attacker to the Druid. Never governed and never held: an ally is dying.
    /// An attacker in range but out of sight is passed over for the next one in
    /// sight; one beyond range is still picked and refused, as it always was.
    /// The kill target, a target already under hard crowd control and one
    /// immune to Cyclone by diminishing returns are never picked.
    fn try_cyclone_peel(
        &mut self,
        combatant: &mut Combatant,
        focus: Option<(Entity, f32)>,
    ) -> CyclonePeel {
        let kill_target = combatant.target;
        let usable = |turn: &Self, e: &CombatantInfo, category: DRCategory| {
            e.is_alive
                && Some(e.entity) != kill_target
                && !turn.ctx.is_ccd(e.entity)
                && !turn.ctx.is_dr_immune(e.entity, category)
        };
        // Enemies attacking `ally` from within their reach, nearest first.
        let attackers_of = |turn: &Self, ally: Entity| -> Vec<(Entity, Vec3)> {
            let Some(ally_pos) = turn.ctx.combatants.get(&ally).map(|a| a.position) else {
                return Vec::new();
            };
            let mut attackers: Vec<&CombatantInfo> = turn
                .ctx
                .enemies_targeting(ally)
                .into_iter()
                .filter(|e| {
                    let reach = match e.pet_type {
                        Some(pet) => pet.preferred_range(),
                        None => e.class.preferred_range(),
                    } + DRUID_ATTACK_RANGE_SLACK;
                    e.position.distance(ally_pos) <= reach
                })
                .collect();
            attackers.sort_by(|a, b| {
                turn.my_pos
                    .distance(a.position)
                    .partial_cmp(&turn.my_pos.distance(b.position))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            attackers
                .into_iter()
                .map(|e| (e.entity, e.position))
                .collect()
        };

        // An enemy a cast of `ability` could reach but for sight
        // ([`super::sight_blocks`]) is passed over for the next one, so an
        // occluded peel pick yields to one in sight; a pick beyond range is
        // still picked and refused, as it always was.
        let cyclone_range = self.abilities.get_unchecked(&AbilityType::Cyclone).range;
        let mut cyclone_occluded = false;

        // The attacker to cyclone.
        let peel = focus
            .filter(|&(_, hp)| hp < DRUID_URGENT_HP)
            .and_then(|(ally, _)| {
                attackers_of(self, ally).into_iter().find(|&(e, pos)| {
                    let eligible = self.ctx.combatants.get(&e).is_some_and(|info| {
                        !info.is_pet && usable(self, info, DRCategory::Cyclone)
                    });
                    let blocked =
                        eligible && super::sight_blocks(self.ctx, cyclone_range, self.my_pos, pos);
                    cyclone_occluded |= blocked;
                    eligible && !blocked
                })
            });
        let mut verdict = CyclonePeel {
            cast: false,
            picked: peel.is_some(),
            occluded: cyclone_occluded,
        };
        if let Some((enemy, enemy_pos)) = peel {
            verdict.cast = self.cast(combatant, AbilityType::Cyclone, enemy, enemy_pos);
        } else if cyclone_occluded {
            // Never governed: a peel is for a dying ally.
            let reason =
                self.unreached(combatant, AbilityType::Cyclone, RejectionReason::LosBlocked);
            self.builder.reject(AbilityType::Cyclone, reason);
        }
        verdict
    }

    /// The Roots PEEL: Entangling Roots on a melee enemy or pet attacking or
    /// closing on a teammate below `urgency_hp_threshold` within heal range
    /// (the Druid included; in sight or not — the melee may be in sight when
    /// the teammate is not), lowest teammate first. The pick is reach-first
    /// ([`RootsPick::Peel`]): an attacker Roots cannot land on yields to the
    /// next one, then to the next dying teammate's. Never governed and never
    /// held for the medic walk. Silent when there is nothing to peel: step 8's
    /// Roots traces the rejection.
    fn try_roots_peel(&mut self, combatant: &mut Combatant) -> bool {
        // A Druid that cannot cast Roots at all leaves the refusal to step 8,
        // so a frame carries one Roots rejection, not two.
        if self
            .caster_refusal(combatant, AbilityType::EntanglingRoots)
            .is_some()
        {
            return false;
        }
        let mut dying: Vec<&CombatantInfo> = self
            .ctx
            .alive_allies()
            .into_iter()
            .filter(|a| {
                !a.is_pet
                    && !self.ctx.is_cycloned(a.entity)
                    && a.health_pct() < self.urgency_hp
                    && self.my_pos.distance(a.position) <= self.heal_range
            })
            .collect();
        dying.sort_by(|a, b| {
            a.health_pct()
                .partial_cmp(&b.health_pct())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let dying: Vec<Entity> = dying.into_iter().map(|a| a.entity).collect();
        let Some((enemy, enemy_pos)) = self.melee_threat(
            combatant.target,
            &dying,
            RootsPick::Peel,
            &mut RootsSkipped::default(),
        ) else {
            return false;
        };
        self.cast(combatant, AbilityType::EntanglingRoots, enemy, enemy_pos)
    }

    /// The Roots pick: for the first of `guarded` that has one, the visible
    /// melee enemy or pet targeting it within `threat_intent_radius` — nearest
    /// to it first — that Roots can take (alive, not the kill target, not
    /// already crowd-controlled, not immune by diminishing returns). A
    /// candidate Roots would refuse for the target alone is passed over for
    /// the next one, then for the next guarded ally's, and noted in `skipped`:
    /// one immune to damage ([`CombatContext::entity_is_immune`]), and one in
    /// range but out of sight. `mode` says what happens to one beyond range.
    fn melee_threat(
        &self,
        kill_target: Option<Entity>,
        guarded: &[Entity],
        mode: RootsPick,
        skipped: &mut RootsSkipped,
    ) -> Option<(Entity, Vec3)> {
        let roots_range = self
            .abilities
            .get_unchecked(&AbilityType::EntanglingRoots)
            .range;
        guarded.iter().find_map(|&ally| {
            let ally_pos = self.ctx.combatants.get(&ally)?.position;
            self.ctx
                .enemies_targeting(ally)
                .into_iter()
                .filter(|e| {
                    (e.class.is_melee() || e.is_pet)
                        && e.position.distance(ally_pos) <= self.threat_radius
                        && e.is_alive
                        && Some(e.entity) != kill_target
                        && !self.ctx.is_ccd(e.entity)
                        && !self.ctx.is_dr_immune(e.entity, DRCategory::Roots)
                })
                .filter(|e| {
                    let reach = super::cast_reach(self.ctx, roots_range, self.my_pos, e.position);
                    let passed_over = match reach {
                        super::CastReach::Reaches => false,
                        super::CastReach::LosBlocked => {
                            skipped.occluded = true;
                            true
                        }
                        super::CastReach::OutOfRange { .. } => mode == RootsPick::Peel,
                    };
                    if passed_over {
                        return false;
                    }
                    let immune = self.ctx.entity_is_immune(e.entity);
                    skipped.immune |= immune;
                    !immune
                })
                .min_by(|a, b| {
                    a.position
                        .distance(ally_pos)
                        .partial_cmp(&b.position.distance(ally_pos))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|e| (e.entity, e.position))
        })
    }

    /// Travel Form, when [`shift_trigger`] says so. An instant: the mana and
    /// the global cooldown are paid here, and `effects::process_travel_form`
    /// breaks the roots and snares and puts the form on next frame.
    fn try_travel_form(&mut self, combatant: &mut Combatant) -> bool {
        let ability = AbilityType::TravelForm;
        let view = ShiftView::of(
            self.ctx,
            self.entity,
            self.my_pos,
            self.auras,
            self.pressured,
            self.threat_radius,
            combatant.current_health / combatant.max_health,
        );
        if shift_trigger(view).is_none() {
            self.builder.reject(
                ability,
                RejectionReason::PreconditionUnmet {
                    note: "no threat to escape: not pressured, unsnared or unchased, or healthy"
                        .to_string(),
                },
            );
            return false;
        }
        let def = self.abilities.get_unchecked(&ability);
        let opts = PreCastOpts::default();
        if !pre_cast_ok(
            ability,
            def,
            combatant,
            self.my_pos,
            self.auras,
            None,
            self.ctx,
            opts,
        ) {
            self.builder.reject(
                ability,
                classify_pre_cast_failure(
                    ability,
                    def,
                    combatant,
                    self.my_pos,
                    self.auras,
                    None,
                    self.ctx,
                    opts,
                ),
            );
            return false;
        }
        self.builder.choose(ability, Some(self.entity), true);
        combatant.current_mana -= def.mana_cost;
        combatant.global_cooldown = GCD;
        self.log_use(combatant, &def.name, self.entity, "casts");
        self.commands.spawn(ShapeshiftPending {
            caster: self.entity,
            shift: Shift::IntoTravelForm,
        });
        true
    }

    /// Crowd control, in priority order:
    /// 1. **Cyclone as a peel** — the focused ally is below [`DRUID_URGENT_HP`]
    ///    and a visible enemy is attacking it: cyclone the attacker. Never
    ///    governed: an ally is dying.
    /// 2. **Cyclone on the enemy healer** — the kill target is below
    ///    [`DRUID_CYCLONE_KILL_HP`]: six seconds with no heals on it.
    /// 3. **Entangling Roots** — a melee enemy or pet attacking the Druid or
    ///    the focused ally (or closing on either) is pinned. One immune to
    ///    damage, or in range but out of sight, is passed over for the next;
    ///    one beyond range is still picked and refused ([`RootsPick::Step8`]).
    ///
    /// The kill target is never cycloned or rooted: a Cyclone would make it
    /// immune to the team's damage, and the team's damage would break a root.
    /// A target already under hard crowd control, or immune to the bucket by
    /// diminishing returns, is passed over, and so is a peel pick in range but
    /// out of sight: the next attacker in sight is taken instead.
    ///
    /// The Cyclone peel (1) is made at step 7 ([`Self::try_cyclone_peel`]);
    /// `cyclone_peel` is its verdict. While `held`
    /// ([`Self::held_for_medic_walk`]) the rest is held.
    fn try_control(
        &mut self,
        combatant: &mut Combatant,
        focus: Option<(Entity, f32)>,
        held: bool,
        cyclone_peel: CyclonePeel,
    ) -> bool {
        let kill_target = combatant.target;
        let usable = |turn: &Self, e: &CombatantInfo, category: DRCategory| {
            e.is_alive
                && Some(e.entity) != kill_target
                && !turn.ctx.is_ccd(e.entity)
                && !turn.ctx.is_dr_immune(e.entity, category)
        };
        // 2. Cyclone the enemy healer when the kill is close.
        let kill_close = kill_target
            .and_then(|t| self.ctx.combatants.get(&t))
            .is_some_and(|t| t.is_alive && t.health_pct() < DRUID_CYCLONE_KILL_HP);
        let healer = self
            .ctx
            .enemy_healer()
            .and_then(|h| self.ctx.combatants.get(&h))
            .filter(|h| usable(self, h, DRCategory::Cyclone))
            .map(|h| (h.entity, h.position));
        match healer {
            Some(_) if kill_close && held => {
                self.builder
                    .reject(AbilityType::Cyclone, Self::held_reason());
            }
            Some((healer, healer_pos)) if kill_close => {
                if self.governed_cast(combatant, AbilityType::Cyclone, healer, healer_pos, false) {
                    return true;
                }
            }
            _ if !cyclone_peel.picked && !cyclone_peel.occluded => {
                self.builder.reject(
                    AbilityType::Cyclone,
                    RejectionReason::PreconditionUnmet {
                        note: "no dying ally to peel for, and no enemy healer to cyclone \
                               while the kill is close"
                            .to_string(),
                    },
                );
            }
            _ => {}
        }

        // 3. Entangling Roots on a melee or pet on the Druid or its focus.
        let mut guarded = vec![self.entity];
        if let Some((ally, _)) = focus {
            guarded.push(ally);
        }
        let mut skipped = RootsSkipped::default();
        let melee_threat = self.melee_threat(kill_target, &guarded, RootsPick::Step8, &mut skipped);
        let urgent = focus.is_some_and(|(_, hp)| hp < DRUID_URGENT_HP);
        let Some((enemy, enemy_pos)) = melee_threat else {
            let roots = AbilityType::EntanglingRoots;
            // A passed-over pick reports what a cast on it would have met
            // first: the governor, then the caster's own state, then the
            // target — immune (the pick in sight), else out of sight.
            let reason = if skipped.immune || skipped.occluded {
                let target = if skipped.immune {
                    RejectionReason::TargetImmune
                } else {
                    RejectionReason::LosBlocked
                };
                self.governor_refusal(combatant, roots, urgent)
                    .unwrap_or_else(|| self.caster_refusal(combatant, roots).unwrap_or(target))
            } else {
                RejectionReason::NoValidTarget
            };
            self.builder.reject(roots, reason);
            return false;
        };
        if held {
            self.builder
                .reject(AbilityType::EntanglingRoots, Self::held_reason());
            return false;
        }
        self.governed_cast(
            combatant,
            AbilityType::EntanglingRoots,
            enemy,
            enemy_pos,
            urgent,
        )
    }

    /// Mark of the Wild on the first ally (self included, pets excluded) who
    /// does not carry it. Applied directly, like Power Word: Fortitude: a
    /// pre-match buff has no cast to resolve.
    ///
    /// An ally another Druid has already sent a Mark to THIS frame counts as
    /// carrying it (`marked_this_frame`): the Mark lands next frame, so the
    /// aura snapshot cannot show it yet, and a second Mark would only refresh
    /// the first — 30 mana for nothing. The Paladin's `paladin_aura_this_frame`
    /// is the same guard.
    ///
    /// While `held` ([`Self::held_for_medic_walk`]) an ally who needs the Mark
    /// is refused it for now.
    fn try_mark_of_the_wild(&mut self, combatant: &mut Combatant, held: bool) -> bool {
        let ability = AbilityType::MarkOfTheWild;
        let mark_range = self.abilities.get_unchecked(&ability).range;
        let mut occluded = false;
        let unbuffed = self
            .ctx
            .alive_allies()
            .into_iter()
            .filter(|a| {
                !self.ctx.is_cycloned(a.entity)
                    && !self.marked_this_frame.contains(&a.entity)
                    && !self.ctx.active_auras.get(&a.entity).is_some_and(|auras| {
                        auras
                            .iter()
                            .any(|aura| aura.compound == Some(CompoundDebuff::MarkOfTheWild))
                    })
            })
            // An ally in range but out of sight yields to the next one in sight.
            .find(|a| {
                let blocked = super::sight_blocks(self.ctx, mark_range, self.my_pos, a.position);
                occluded |= blocked;
                !blocked
            })
            .map(|a| (a.entity, a.position));
        let Some((target, target_pos)) = unbuffed else {
            let reason = if occluded {
                self.unreached(combatant, ability, RejectionReason::LosBlocked)
            } else {
                RejectionReason::AlreadyApplied
            };
            self.builder.reject(ability, reason);
            return false;
        };
        if held {
            self.builder.reject(ability, Self::held_reason());
            return false;
        }
        if !self.guard(combatant, ability, target, target_pos) {
            return false;
        }

        let def = self.abilities.get_unchecked(&ability);
        self.builder.choose(ability, Some(target), true);
        combatant.current_mana -= def.mana_cost;
        combatant.global_cooldown = GCD;
        self.log_use(combatant, &def.name, target, "casts");
        self.marked_this_frame.insert(target);
        if let Some(pending) = AuraPending::from_ability(target, self.entity, def) {
            self.commands.spawn(pending);
        }
        true
    }

    /// Moonfire on the kill target: governed, and skipped while its DoT is
    /// still up or the target is under our own breakable crowd control — and
    /// while `held` ([`Self::held_for_medic_walk`]).
    fn try_moonfire(&mut self, combatant: &mut Combatant, held: bool) -> bool {
        let ability = AbilityType::Moonfire;
        let Some(target) = combatant.target else {
            self.builder.reject(ability, RejectionReason::NoValidTarget);
            return false;
        };
        let Some(target_pos) = self
            .ctx
            .combatants
            .get(&target)
            .filter(|i| i.is_alive)
            .map(|i| i.position)
        else {
            self.builder.reject(ability, RejectionReason::NoValidTarget);
            return false;
        };
        if self.has_own(target, ability) {
            self.builder
                .reject(ability, RejectionReason::AlreadyApplied);
            return false;
        }
        if held {
            self.builder.reject(ability, Self::held_reason());
            return false;
        }
        self.governed_cast(combatant, ability, target, target_pos, false)
    }

    /// [`Self::cast`], but a proactive cast (`urgent` false) must first clear
    /// the mana governor.
    fn governed_cast(
        &mut self,
        combatant: &mut Combatant,
        ability: AbilityType,
        target: Entity,
        target_pos: Vec3,
        urgent: bool,
    ) -> bool {
        if let Some(reason) = self.governor_refusal(combatant, ability, urgent) {
            self.builder.reject(ability, reason);
            return false;
        }
        self.cast(combatant, ability, target, target_pos)
    }

    /// The mana governor's refusal of a proactive cast (`urgent` false), if it
    /// refuses.
    fn governor_refusal(
        &self,
        combatant: &Combatant,
        ability: AbilityType,
        urgent: bool,
    ) -> Option<RejectionReason> {
        let cost = self.abilities.get_unchecked(&ability).mana_cost;
        (!urgent && !governor_allows(combatant, cost, self.time_since_gates)).then(|| {
            RejectionReason::PreconditionUnmet {
                note: "mana governor: holding the reserve for dampening".to_string(),
            }
        })
    }

    /// The caster's own refusal of `ability` — Travel Form, a lockout, a
    /// silence, a cooldown, mana — asked with no target, in `pre_cast_ok`'s
    /// order. `None` when the caster could cast it.
    fn caster_refusal(
        &self,
        combatant: &Combatant,
        ability: AbilityType,
    ) -> Option<RejectionReason> {
        let def = self.abilities.get_unchecked(&ability);
        let opts = Self::guard_opts(ability);
        (!pre_cast_ok(
            ability,
            def,
            combatant,
            self.my_pos,
            self.auras,
            None,
            self.ctx,
            opts,
        ))
        .then(|| {
            classify_pre_cast_failure(
                ability,
                def,
                combatant,
                self.my_pos,
                self.auras,
                None,
                self.ctx,
                opts,
            )
        })
    }

    /// [`unreached_reason`] for a Druid pick that found no target in reach:
    /// a `LosBlocked` verdict yields to the caster's own refusal, with the
    /// guard's own options.
    fn unreached(
        &self,
        combatant: &Combatant,
        ability: AbilityType,
        reason: RejectionReason,
    ) -> RejectionReason {
        let def = self.abilities.get_unchecked(&ability);
        unreached_reason(
            reason,
            ability,
            def,
            combatant,
            self.my_pos,
            self.auras,
            self.ctx,
            Self::guard_opts(ability),
        )
    }

    /// The opt-in pre-cast guards each Druid cast runs ([`Self::guard`]).
    fn guard_opts(ability: AbilityType) -> PreCastOpts {
        match ability {
            AbilityType::Moonfire => PreCastOpts {
                check_friendly_cc: true,
                check_target_immune: true,
                ..Default::default()
            },
            AbilityType::EntanglingRoots | AbilityType::Cyclone => PreCastOpts {
                check_target_immune: true,
                ..Default::default()
            },
            _ => PreCastOpts::default(),
        }
    }

    /// Cast an instant through a zero-length `CastingState`, so the generic
    /// completion path lands it — heal, damage, aura, the Swiftmend
    /// consumption and the mana charge alike. The route Frost Shock takes.
    fn cast(
        &mut self,
        combatant: &mut Combatant,
        ability: AbilityType,
        target: Entity,
        target_pos: Vec3,
    ) -> bool {
        if !self.guard(combatant, ability, target, target_pos) {
            return false;
        }
        let def = self.abilities.get_unchecked(&ability);
        self.builder.choose(ability, Some(target), true);
        if def.cooldown > 0.0 {
            combatant.start_cooldown(ability, self.abilities);
        }
        combatant.global_cooldown = GCD;
        let cast_time = calculate_cast_time(def.cast_time, self.auras);
        self.commands
            .entity(self.entity)
            .insert(CastingState::new(ability, target, cast_time));
        self.log_use(combatant, &def.name, target, "casts");
        true
    }

    /// The shared pre-cast guards, with the rejection traced on failure.
    fn guard(
        &mut self,
        combatant: &Combatant,
        ability: AbilityType,
        target: Entity,
        target_pos: Vec3,
    ) -> bool {
        let def = self.abilities.get_unchecked(&ability);
        let opts = Self::guard_opts(ability);
        let target = Some((target, target_pos));
        if pre_cast_ok(
            ability,
            def,
            combatant,
            self.my_pos,
            self.auras,
            target,
            self.ctx,
            opts,
        ) {
            return true;
        }
        self.builder.reject(
            ability,
            classify_pre_cast_failure(
                ability,
                def,
                combatant,
                self.my_pos,
                self.auras,
                target,
                self.ctx,
                opts,
            ),
        );
        false
    }

    fn own(&self, target: Entity, ability: AbilityType) -> Option<&Aura> {
        own_aura(self.ctx, self.abilities, self.entity, target, ability)
    }

    fn has_own(&self, target: Entity, ability: AbilityType) -> bool {
        self.own(target, ability).is_some()
    }

    fn log_use(&mut self, combatant: &Combatant, name: &str, target: Entity, verb: &str) {
        let target_id = self.ctx.combatants.get(&target).map(|info| info.log_id());
        log_ability_use(
            self.combat_log,
            combatant.team,
            combatant.slot,
            combatant.class,
            name,
            target_id,
            verb,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lifebloom(count: u8, duration: f32) -> Aura {
        Aura {
            effect_type: AuraType::HealingOverTime,
            duration,
            stacks: Some(AuraStacks {
                count,
                max: 3,
                scope: StackScope::PerCaster,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn lifebloom_waits_for_the_ally_to_be_attacked() {
        assert_eq!(
            lifebloom_decision(None, 1.0, false),
            LifebloomDecision::Hold
        );
        assert_eq!(
            lifebloom_decision(Some(&lifebloom(3, 1.0)), 0.95, false),
            LifebloomDecision::Hold,
            "a full stack on an ally nobody is hitting is not rolled"
        );
    }

    #[test]
    fn lifebloom_builds_its_stack_first() {
        assert_eq!(lifebloom_decision(None, 1.0, true), LifebloomDecision::Cast);
        assert_eq!(
            lifebloom_decision(Some(&lifebloom(1, 6.0)), 1.0, true),
            LifebloomDecision::Cast
        );
        assert_eq!(
            lifebloom_decision(Some(&lifebloom(2, 0.5)), 0.2, true),
            LifebloomDecision::Cast,
            "a partial stack is always built up, even on a hurt ally"
        );
    }

    #[test]
    fn a_full_stack_holds_until_its_decision_point() {
        assert_eq!(
            lifebloom_decision(Some(&lifebloom(3, 5.0)), 1.0, true),
            LifebloomDecision::Hold
        );
        assert_eq!(
            lifebloom_decision(Some(&lifebloom(3, 5.0)), 0.3, true),
            LifebloomDecision::Hold
        );
    }

    #[test]
    fn at_the_decision_point_it_rolls_on_a_healthy_ally_and_blooms_on_a_hurt_one() {
        assert_eq!(
            lifebloom_decision(Some(&lifebloom(3, 1.0)), 0.95, true),
            LifebloomDecision::Cast,
            "healthy: refresh and keep it rolling — the bloom would be overheal"
        );
        assert_eq!(
            lifebloom_decision(Some(&lifebloom(3, 1.0)), 0.5, true),
            LifebloomDecision::LetBloom,
            "hurt: let the bloom land rather than refresh it away"
        );
    }

    fn shift() -> ShiftView {
        ShiftView {
            pressured: true,
            rooted: false,
            snared: false,
            chaser: false,
            melee_on_me: false,
            self_health_pct: 1.0,
        }
    }

    #[test]
    fn the_shift_answers_a_threat_only() {
        // Not pressured: nothing, rooted or not.
        assert_eq!(
            shift_trigger(ShiftView {
                pressured: false,
                rooted: true,
                ..shift()
            }),
            None
        );
        // Pressured and rooted: break it.
        assert_eq!(
            shift_trigger(ShiftView {
                rooted: true,
                ..shift()
            }),
            Some(ShiftReason::BreakImpairment)
        );
        // Snared with a melee or pet chasing: break it...
        assert_eq!(
            shift_trigger(ShiftView {
                snared: true,
                chaser: true,
                ..shift()
            }),
            Some(ShiftReason::BreakImpairment)
        );
        // ...but a snare with no chaser is shrugged off: the form outruns
        // legs, not spells.
        assert_eq!(
            shift_trigger(ShiftView {
                snared: true,
                ..shift()
            }),
            None
        );
        // Pressured, free, a melee on a hurt Druid: open distance.
        assert_eq!(
            shift_trigger(ShiftView {
                melee_on_me: true,
                self_health_pct: DRUID_SHIFT_OPEN_HP - 0.01,
                ..shift()
            }),
            Some(ShiftReason::OpenDistance)
        );
        // ...but not on a healthy one, and not with no melee on it.
        assert_eq!(
            shift_trigger(ShiftView {
                melee_on_me: true,
                self_health_pct: DRUID_SHIFT_OPEN_HP,
                ..shift()
            }),
            None
        );
        assert_eq!(
            shift_trigger(ShiftView {
                self_health_pct: 0.1,
                ..shift()
            }),
            None
        );
    }

    fn view() -> FormView {
        FormView {
            in_form_secs: DRUID_MIN_FORM_SECS,
            rooted: false,
            chaser_near: false,
            chaser_striking: false,
            healing_needed: true,
            enemy_in_reach: false,
            would_reshift: false,
        }
    }

    #[test]
    fn the_druid_leaves_the_form_once_safe_with_work_to_do() {
        assert!(should_leave_form(view()));
        assert!(should_leave_form(FormView {
            healing_needed: false,
            enemy_in_reach: true,
            ..view()
        }));
        assert!(
            !should_leave_form(FormView {
                in_form_secs: DRUID_MIN_FORM_SECS - 0.1,
                ..view()
            }),
            "a fresh shift is not thrown straight back"
        );
        assert!(
            !should_leave_form(FormView {
                chaser_near: true,
                ..view()
            }),
            "a chaser inside the danger radius keeps it shifted"
        );
        assert!(
            should_leave_form(FormView {
                chaser_near: true,
                in_form_secs: DRUID_MAX_CHASE_SECS,
                ..view()
            }),
            "...until it has run too long without shaking it"
        );
        assert!(
            !should_leave_form(FormView {
                chaser_near: true,
                chaser_striking: true,
                in_form_secs: DRUID_MAX_CHASE_SECS,
                ..view()
            }),
            "never with the chaser in striking reach"
        );
        assert!(
            !should_leave_form(FormView {
                healing_needed: false,
                ..view()
            }),
            "nothing to heal and nothing in reach: stay fast"
        );
        assert!(
            should_leave_form(FormView {
                in_form_secs: 0.1,
                chaser_near: true,
                rooted: true,
                ..view()
            }),
            "rooted again: leave at once, to re-shift"
        );
        assert!(
            !should_leave_form(FormView {
                would_reshift: true,
                ..view()
            }),
            "never into a shift straight back"
        );
    }

    #[test]
    fn the_governor_reserve_shrinks_to_nothing_by_half_dampening() {
        assert_eq!(mana_reserve(300.0, 0.0), 150.0);
        let halfway = mana_reserve(300.0, GOVERNOR_HORIZON_SECS * 0.5);
        assert!((halfway - 75.0).abs() < 1e-3, "{halfway}");
        assert_eq!(mana_reserve(300.0, GOVERNOR_HORIZON_SECS), 0.0);
        assert_eq!(mana_reserve(300.0, GOVERNOR_HORIZON_SECS * 2.0), 0.0);
        assert_eq!(
            GOVERNOR_HORIZON_SECS,
            DAMPENING_START_SECS + DAMPENING_RAMP_SECS * 0.5,
            "the horizon is the moment dampening reaches 50%"
        );
    }
}

// ----------------------------------------------------------------------------
// AS-202: the Druid's heals choose among allies in sight
// ----------------------------------------------------------------------------

#[cfg(test)]
mod reach_tests {
    use super::*;
    use crate::states::match_config::CharacterClass;
    use crate::states::match_config::CharacterClass::{Druid, Mage, Rogue, Warrior};
    use crate::states::play_match::class_ai::reach_fixture::*;
    use crate::states::play_match::map_geometry::ObstacleVolume;

    /// The Druid with a Warrior `BEHIND` the pillar and a Mage `IN_SIGHT`,
    /// plus one enemy Rogue beside, and attacking, each ally `attacked` names
    /// (1 the Warrior, 2 the Mage).
    fn scene(warrior_hp: f32, mage_hp: f32, attacked: &[usize]) -> ReachScene {
        let mut units = vec![
            (1, Warrior, BEHIND, warrior_hp),
            (1, Mage, IN_SIGHT, mage_hp),
        ];
        for &ally in attacked {
            let at = if ally == 1 { BEHIND } else { IN_SIGHT };
            units.push((2, Rogue, at + Vec3::new(1.0, 0.0, 1.0), 1.0));
        }
        let mut s = ReachScene::new(Druid, &units);
        for (i, &ally) in attacked.iter().enumerate() {
            let (rogue, target) = (s.units[3 + i], s.units[ally]);
            s.roster.get_mut(&rogue).unwrap().target = Some(target);
        }
        s
    }

    /// One Druid turn: `step` runs against a [`DruidTurn`] over the scene,
    /// with the medic walk walking to `medic_walk`.
    fn turn(
        s: &mut ReachScene,
        obstacles: &[ObstacleVolume],
        time_since_gates: f32,
        medic_walk: Option<Entity>,
        step: impl FnOnce(&mut DruidTurn, &mut Combatant) -> bool,
    ) -> DecisionTrace {
        let abilities = AbilityDefinitions::default();
        let movement = MovementConfig::default();
        let me = s.units[0];
        let mut marked = HashSet::new();
        s.run(obstacles, |commands, ctx, combatant, builder| {
            let mut log = CombatLog::default();
            let mut turn = DruidTurn {
                commands,
                combat_log: &mut log,
                abilities: &abilities,
                entity: me,
                my_pos: CASTER,
                auras: None,
                ctx,
                heal_range: movement.shared.heal_range,
                threat_radius: movement.shared.threat_intent_radius,
                urgency_hp: movement.shared.urgency_hp_threshold,
                medic_walk,
                pressured: false,
                time_since_gates,
                marked_this_frame: &mut marked,
                builder,
            };
            step(&mut turn, combatant)
        })
    }

    /// One rotation turn (everything after the pre-match buff).
    fn rotation(s: &mut ReachScene, obstacles: &[ObstacleVolume]) -> DecisionTrace {
        turn(s, obstacles, 20.0, None, |t, c| t.rotation(c, false))
    }

    fn mark(s: &mut ReachScene, obstacles: &[ObstacleVolume]) -> DecisionTrace {
        turn(s, obstacles, 0.0, None, |t, c| {
            t.try_mark_of_the_wild(c, false)
        })
    }

    /// A fresh Rejuvenation `by` cast, well clear of its refresh window. A
    /// Rejuvenation is its caster's own (`StackScope::PerCaster`), so the
    /// Druid under test must have cast it for its rotation to count it.
    fn rejuvenation(by: Entity) -> Vec<Aura> {
        vec![Aura {
            effect_type: AuraType::HealingOverTime,
            ability_name: "Rejuvenation".to_string(),
            duration: 12.0,
            caster: Some(by),
            ..Default::default()
        }]
    }

    /// The emergency step (Swiftmend, or the Rejuvenation that arms it) goes
    /// to the lowest DYING ally it can see. The Mage carries a Rejuvenation,
    /// so the emergency step Swiftmends it, and the top-up (which skips an
    /// ally with one) cannot be what heals it.
    #[test]
    fn the_emergency_heal_falls_back_to_a_dying_ally_in_sight() {
        let mut s = scene(0.3, 0.4, &[]);
        let (warrior, mage) = (s.units[1], s.units[2]);
        s.auras.insert(mage, rejuvenation(s.units[0]));
        assert_eq!(
            outcome(&rotation(&mut s, &[])),
            chose("Rejuvenation", warrior),
            "no pillar: the lowest dying ally"
        );
        assert_eq!(
            outcome(&rotation(&mut s, &pillar())),
            chose("Swiftmend", mage),
            "the occluded Warrior yields to the dying Mage in sight"
        );

        let mut s = scene(0.4, 0.3, &[]);
        let mage = s.units[2];
        s.auras.insert(mage, rejuvenation(s.units[0]));
        assert_eq!(
            outcome(&rotation(&mut s, &pillar())),
            chose("Swiftmend", mage),
            "a dying ally in sight is healed with the pillar standing"
        );

        let mut s = scene(0.3, 1.0, &[]);
        let trace = rotation(&mut s, &pillar());
        assert_eq!(outcome(&trace), None, "no heal through the pillar");
        assert_eq!(candidate(&trace, "Swiftmend")["reason"], "LosBlocked");
    }

    /// The focus heals go to the most-attacked ally the Druid can see. Both
    /// allies carry a Rejuvenation, so the focus is rolled a Lifebloom — a
    /// cast only the focus step makes.
    #[test]
    fn the_focus_heals_fall_back_to_an_attacked_ally_in_sight() {
        let scene_rejuvenated = |warrior_hp, mage_hp| {
            // Both attacked once: the lower one is the focus.
            let mut s = scene(warrior_hp, mage_hp, &[1, 2]);
            for ally in [s.units[1], s.units[2]] {
                s.auras.insert(ally, rejuvenation(s.units[0]));
            }
            s
        };
        let mut s = scene_rejuvenated(0.7, 0.75);
        let (warrior, mage) = (s.units[1], s.units[2]);
        assert_eq!(
            outcome(&rotation(&mut s, &[])),
            chose("Lifebloom", warrior),
            "no pillar: the focus is the lower attacked ally"
        );
        assert_eq!(
            outcome(&rotation(&mut s, &pillar())),
            chose("Lifebloom", mage),
            "the occluded Warrior yields to the attacked Mage in sight"
        );

        let mut s = scene_rejuvenated(0.75, 0.7);
        let mage = s.units[2];
        assert_eq!(
            outcome(&rotation(&mut s, &pillar())),
            chose("Lifebloom", mage),
            "a focus in sight is healed with the pillar standing"
        );
    }

    /// The top-up Rejuvenation goes to the lowest hurt ally the Druid can see.
    #[test]
    fn the_top_up_falls_back_to_a_hurt_ally_in_sight() {
        let mut s = scene(0.6, 0.7, &[]);
        let (warrior, mage) = (s.units[1], s.units[2]);
        assert_eq!(
            outcome(&rotation(&mut s, &[])),
            chose("Rejuvenation", warrior),
            "no pillar: the lowest hurt ally"
        );
        assert_eq!(
            outcome(&rotation(&mut s, &pillar())),
            chose("Rejuvenation", mage),
            "the occluded Warrior yields to the hurt Mage in sight"
        );

        let mut s = scene(0.7, 0.6, &[]);
        let mage = s.units[2];
        assert_eq!(
            outcome(&rotation(&mut s, &pillar())),
            chose("Rejuvenation", mage),
            "a lowest hurt ally in sight is topped up with the pillar standing"
        );
    }

    /// Mark of the Wild goes to the first unmarked ally the Druid can see.
    #[test]
    fn mark_of_the_wild_falls_back_to_the_next_unmarked_ally_in_sight() {
        let marked = || {
            vec![Aura {
                effect_type: AuraType::MaxHealthIncrease,
                compound: Some(CompoundDebuff::MarkOfTheWild),
                ..Default::default()
            }]
        };
        let mut s = scene(1.0, 1.0, &[]);
        let (me, warrior, mage) = (s.units[0], s.units[1], s.units[2]);
        s.auras.insert(me, marked());
        assert_eq!(
            outcome(&mark(&mut s, &[])),
            chose("MarkOfTheWild", warrior),
            "no pillar: the first unmarked ally"
        );
        assert_eq!(
            outcome(&mark(&mut s, &pillar())),
            chose("MarkOfTheWild", mage),
            "the occluded Warrior yields to the Mage in sight"
        );

        s.auras.insert(warrior, marked());
        assert_eq!(
            outcome(&mark(&mut s, &pillar())),
            chose("MarkOfTheWild", mage),
            "a first unmarked ally in sight is marked with the pillar standing"
        );

        s.auras.remove(&warrior);
        s.auras.insert(mage, marked());
        let trace = mark(&mut s, &pillar());
        assert_eq!(outcome(&trace), None, "no Mark through the pillar");
        assert_eq!(candidate(&trace, "MarkOfTheWild")["reason"], "LosBlocked");
    }

    // ------------------------------------------------------------------------
    // AS-204: the Cyclone and Entangling Roots peels choose among attackers
    // in sight
    // ------------------------------------------------------------------------

    /// Nearer to the Druid than [`SIGHTED_ATTACKER`], with the pillar between.
    const OCCLUDED_ATTACKER: Vec3 = Vec3::new(12.0, 1.0, 0.0);
    /// Clear of the pillar, and in Cyclone's 20yd range.
    const SIGHTED_ATTACKER: Vec3 = Vec3::new(12.0, 1.0, 6.0);
    /// The dying ally both attackers are on, within a Rogue's reach of each.
    const PEELED_ALLY: Vec3 = Vec3::new(13.0, 1.0, 3.0);

    /// The Druid, an ally Warrior at `ally` (`ally_hp`), and an enemy Rogue at
    /// each of `at`, every one of them attacking the ally — or the Druid, when
    /// `ally` is `None`. Units: the Druid, then the Warrior if any, then the
    /// Rogues in order.
    fn attacked_scene(ally: Option<(Vec3, f32)>, at: &[Vec3]) -> ReachScene {
        let mut units: Vec<_> = ally
            .map(|(p, hp)| (1, Warrior, p, hp))
            .into_iter()
            .collect();
        units.extend(at.iter().map(|&p| (2, Rogue, p, 1.0)));
        let mut s = ReachScene::new(Druid, &units);
        let victim = s.units[if ally.is_some() { 1 } else { 0 }];
        let first = if ally.is_some() { 2 } else { 1 };
        for rogue in s.units[first..].to_vec() {
            s.roster.get_mut(&rogue).unwrap().target = Some(victim);
        }
        s
    }

    /// One control step with `focus` (the ally and its HP fraction).
    fn control(
        s: &mut ReachScene,
        obstacles: &[ObstacleVolume],
        focus: Option<(Entity, f32)>,
    ) -> DecisionTrace {
        turn(s, obstacles, 20.0, None, |t, c| {
            let peel = t.try_cyclone_peel(c, focus);
            peel.cast || t.try_control(c, focus, false, peel)
        })
    }

    /// The Cyclone peel takes the dying ally's nearest attacker; behind the
    /// pillar it yields to the nearest attacker in sight.
    #[test]
    fn the_cyclone_peel_falls_back_to_an_attacker_in_sight() {
        let mut s = attacked_scene(
            Some((PEELED_ALLY, 0.3)),
            &[OCCLUDED_ATTACKER, SIGHTED_ATTACKER],
        );
        let (ally, occluded, sighted) = (s.units[1], s.units[2], s.units[3]);
        let focus = Some((ally, 0.3));
        assert_eq!(
            outcome(&control(&mut s, &[], focus)),
            chose("Cyclone", occluded),
            "no pillar: the nearest attacker"
        );
        assert_eq!(
            outcome(&control(&mut s, &pillar(), focus)),
            chose("Cyclone", sighted),
            "the occluded attacker yields to the one in sight"
        );
    }

    /// A nearest attacker in sight is cycloned with the pillar standing; one
    /// only the pillar hides is refused `LosBlocked`, by Cyclone and Roots both.
    #[test]
    fn the_cyclone_peel_lands_in_sight_and_is_refused_behind_the_pillar() {
        let mut s = attacked_scene(Some((PEELED_ALLY, 0.3)), &[SIGHTED_ATTACKER]);
        let (ally, sighted) = (s.units[1], s.units[2]);
        assert_eq!(
            outcome(&control(&mut s, &pillar(), Some((ally, 0.3)))),
            chose("Cyclone", sighted),
        );

        let mut s = attacked_scene(Some((PEELED_ALLY, 0.3)), &[OCCLUDED_ATTACKER]);
        let ally = s.units[1];
        let trace = control(&mut s, &pillar(), Some((ally, 0.3)));
        assert_eq!(outcome(&trace), None, "no control through the pillar");
        assert_eq!(candidate(&trace, "Cyclone")["reason"], "LosBlocked");
        assert_eq!(candidate(&trace, "EntanglingRoots")["reason"], "LosBlocked");
    }

    /// Range is unchanged: a nearest attacker beyond Cyclone's range is still
    /// picked and refused out of range, as it always was — with the pillar
    /// between them too, since only an attacker in range is dropped for sight.
    #[test]
    fn the_cyclone_peel_still_refuses_an_attacker_beyond_range() {
        for obstacles in [vec![], pillar()] {
            let mut s = attacked_scene(
                Some((Vec3::new(24.0, 1.0, 0.0), 0.3)),
                &[Vec3::new(22.0, 1.0, 0.0)],
            );
            let ally = s.units[1];
            let trace = control(&mut s, &obstacles, Some((ally, 0.3)));
            assert!(
                candidate(&trace, "Cyclone")["reason"]
                    .get("OutOfRange")
                    .is_some(),
                "{} obstacle(s): {}",
                obstacles.len(),
                candidate(&trace, "Cyclone")
            );
        }
    }

    /// The Roots pick takes the Druid's nearest melee; behind the pillar it
    /// yields to the nearest one in sight.
    #[test]
    fn entangling_roots_falls_back_to_an_attacker_in_sight() {
        let mut s = attacked_scene(None, &[OCCLUDED_ATTACKER, SIGHTED_ATTACKER]);
        let (occluded, sighted) = (s.units[1], s.units[2]);
        assert_eq!(
            outcome(&control(&mut s, &[], None)),
            chose("EntanglingRoots", occluded),
            "no pillar: the nearest attacker"
        );
        assert_eq!(
            outcome(&control(&mut s, &pillar(), None)),
            chose("EntanglingRoots", sighted),
            "the occluded attacker yields to the one in sight"
        );
    }

    /// A nearest melee in sight is rooted with the pillar standing; one only
    /// the pillar hides is refused `LosBlocked`.
    #[test]
    fn entangling_roots_lands_in_sight_and_is_refused_behind_the_pillar() {
        let mut s = attacked_scene(None, &[BEHIND, SIGHTED_ATTACKER]);
        let sighted = s.units[2];
        assert_eq!(
            outcome(&control(&mut s, &pillar(), None)),
            chose("EntanglingRoots", sighted),
        );

        let mut s = attacked_scene(None, &[OCCLUDED_ATTACKER]);
        let trace = control(&mut s, &pillar(), None);
        assert_eq!(outcome(&trace), None, "no Roots through the pillar");
        assert_eq!(candidate(&trace, "EntanglingRoots")["reason"], "LosBlocked");
    }

    /// Range is unchanged: the focus's nearest melee beyond Roots' range is
    /// still picked and refused out of range, not passed over for a farther
    /// one the Druid could reach — with the pillar between the Druid and that
    /// melee too (the farther one stays clear of it), since only a melee in
    /// range is dropped for sight.
    #[test]
    fn entangling_roots_still_refuses_an_attacker_beyond_range() {
        for obstacles in [vec![], pillar()] {
            let mut s = attacked_scene(
                Some((Vec3::new(35.0, 1.0, 0.0), 1.0)),
                &[Vec3::new(34.0, 1.0, 0.0), Vec3::new(28.0, 1.0, 10.0)],
            );
            let ally = s.units[1];
            let trace = control(&mut s, &obstacles, Some((ally, 1.0)));
            assert_eq!(outcome(&trace), None, "{} obstacle(s)", obstacles.len());
            assert!(
                candidate(&trace, "EntanglingRoots")["reason"]
                    .get("OutOfRange")
                    .is_some(),
                "{} obstacle(s): {}",
                obstacles.len(),
                candidate(&trace, "EntanglingRoots")
            );
        }
    }

    // ------------------------------------------------------------------------
    // AS-204: a pick with nothing in sight still reports the caster's refusal
    // ------------------------------------------------------------------------

    /// `ability`'s traced reason in one control step by a Druid at 0 mana.
    fn control_reason_without_mana(
        s: &mut ReachScene,
        obstacles: &[ObstacleVolume],
        focus: Option<(Entity, f32)>,
        ability: &str,
    ) -> serde_json::Value {
        let trace = turn(s, obstacles, 20.0, None, |t, c| {
            c.current_mana = 0.0;
            let peel = t.try_cyclone_peel(c, focus);
            peel.cast || t.try_control(c, focus, false, peel)
        });
        candidate(&trace, ability)["reason"].clone()
    }

    /// A Druid out of mana is refused for mana, whether its only peel pick is
    /// in sight or behind the pillar: sight cannot hide the caster's state.
    #[test]
    fn an_occluded_peel_still_reports_the_druid_out_of_mana() {
        for ability in ["Cyclone", "EntanglingRoots"] {
            let mut s = attacked_scene(Some((PEELED_ALLY, 0.3)), &[OCCLUDED_ATTACKER]);
            let focus = Some((s.units[1], 0.3));
            let open = control_reason_without_mana(&mut s, &[], focus, ability);
            assert!(open.get("InsufficientMana").is_some(), "{ability}: {open}");
            let behind = control_reason_without_mana(&mut s, &pillar(), focus, ability);
            assert_eq!(behind, open, "{ability} behind the pillar");
        }
    }

    /// A proactive Roots (no dying ally) behind the pillar meets the mana
    /// governor first, as a cast on an attacker in sight would.
    #[test]
    fn an_occluded_proactive_roots_still_reports_the_governor() {
        let mut s = attacked_scene(None, &[OCCLUDED_ATTACKER]);
        let open = control_reason_without_mana(&mut s, &[], None, "EntanglingRoots");
        assert_eq!(
            open["PreconditionUnmet"]["note"],
            "mana governor: holding the reserve for dampening"
        );
        let behind = control_reason_without_mana(&mut s, &pillar(), None, "EntanglingRoots");
        assert_eq!(behind, open);
    }

    /// A Druid with Swiftmend on cooldown is refused it for the cooldown,
    /// whether its only dying ally (carrying the Druid's own Rejuvenation to
    /// consume, so the open map reaches the Swiftmend cast) is in sight or
    /// behind the pillar.
    #[test]
    fn an_occluded_emergency_heal_still_reports_swiftmend_on_cooldown() {
        let reason = |obstacles: &[ObstacleVolume]| {
            let mut s = scene(0.3, 1.0, &[]);
            let warrior = s.units[1];
            s.auras.insert(warrior, rejuvenation(s.units[0]));
            s.prep = Some(|c| {
                c.ability_cooldowns.insert(AbilityType::Swiftmend, 3.0);
            });
            candidate(&rotation(&mut s, obstacles), "Swiftmend")["reason"].clone()
        };
        let open = reason(&[]);
        assert!(open.get("OnCooldown").is_some(), "{open}");
        assert_eq!(reason(&pillar()), open, "behind the pillar");
    }

    /// An out-of-mana Druid is refused Mark of the Wild for mana, whether its
    /// only unmarked ally is in sight or behind the pillar.
    #[test]
    fn an_occluded_mark_still_reports_the_druid_out_of_mana() {
        let marked = || {
            vec![Aura {
                effect_type: AuraType::MaxHealthIncrease,
                compound: Some(CompoundDebuff::MarkOfTheWild),
                ..Default::default()
            }]
        };
        let reason = |obstacles: &[ObstacleVolume]| {
            let mut s = scene(1.0, 1.0, &[]);
            let (me, mage) = (s.units[0], s.units[2]);
            s.auras.insert(me, marked());
            s.auras.insert(mage, marked());
            s.prep = Some(|c| c.current_mana = 0.0);
            candidate(&mark(&mut s, obstacles), "MarkOfTheWild")["reason"].clone()
        };
        let open = reason(&[]);
        assert!(open.get("InsufficientMana").is_some(), "{open}");
        assert_eq!(reason(&pillar()), open, "behind the pillar");
    }

    // ------------------------------------------------------------------------
    // AS-210: a dying teammate is healed before any damage or utility
    // ------------------------------------------------------------------------

    /// [`scene`] plus an enemy Rogue 10yd down -Z, in sight of the Druid and
    /// attacking nobody: the Druid's kill target, so Moonfire has a target.
    fn kill_scene(warrior_hp: f32, mage_hp: f32) -> (ReachScene, Entity) {
        let mut s = scene(warrior_hp, mage_hp, &[]);
        let rogue = add_enemy(&mut s, Rogue, Vec3::new(0.0, 1.0, -10.0), None);
        (s, rogue)
    }

    /// An enemy of `class` at `at`, targeting `target`.
    fn add_enemy(
        s: &mut ReachScene,
        class: CharacterClass,
        at: Vec3,
        target: Option<Entity>,
    ) -> Entity {
        let e = s.world.spawn_empty().id();
        s.units.push(e);
        let mut info = unit(e, 2, class, at, 1.0);
        info.target = target;
        s.roster.insert(e, info);
        e
    }

    /// One whole Druid turn after the gates (`DruidTurn::take`) at the kill
    /// target, with the medic walk walking to `medic_walk`.
    fn take(
        s: &mut ReachScene,
        obstacles: &[ObstacleVolume],
        kill: Entity,
        medic_walk: Option<Entity>,
    ) -> DecisionTrace {
        turn(s, obstacles, 20.0, medic_walk, |t, c| {
            c.target = Some(kill);
            t.take(c, true)
        })
    }

    /// This Druid's Lifebloom: `count` stacks with `duration` left.
    fn own_lifebloom(by: Entity, count: u8, duration: f32) -> Aura {
        Aura {
            effect_type: AuraType::HealingOverTime,
            ability_name: "Lifebloom".to_string(),
            duration,
            caster: Some(by),
            stacks: Some(AuraStacks {
                count,
                max: 3,
                scope: StackScope::PerCaster,
            }),
            ..Default::default()
        }
    }

    /// Every heal the Druid has rolling on `ally`: its Rejuvenation and a full
    /// Lifebloom stack, both well clear of their refresh points.
    fn all_heals_rolling(s: &mut ReachScene, ally: Entity) {
        let me = s.units[0];
        let auras = s.auras.entry(ally).or_default();
        auras.extend(rejuvenation(me));
        auras.push(own_lifebloom(me, 3, 8.0));
    }

    /// Everyone carries Mark of the Wild, so the Mark is not what is tested.
    fn mark_everyone(s: &mut ReachScene) {
        for &e in &s.units[..3] {
            s.auras.entry(e).or_default().push(Aura {
                effect_type: AuraType::MaxHealthIncrease,
                compound: Some(CompoundDebuff::MarkOfTheWild),
                ..Default::default()
            });
        }
    }

    /// A Druid whose mana sits under the governor's reserve: every proactive
    /// cast (the focus heals above `DRUID_URGENT_HP`, the top-up, Moonfire) is
    /// refused, so only a heal step 7 makes can land. Above the Innervate
    /// threshold, so Innervate does not take the turn either.
    fn under_the_reserve(c: &mut Combatant) {
        c.current_mana = c.max_mana * 0.4;
    }

    /// The dying Mage carries the Druid's Rejuvenation and is attacked by an
    /// enemy caster; the enemy's attention is on the Warrior, who is the focus
    /// and has every heal rolling. No step before Moonfire used to add to the
    /// Mage — the focus step is the Warrior's, the top-up skips a Rejuvenation
    /// carrier. Step 7 rolls a Lifebloom on it. Attacked by nobody, the Mage
    /// gets no stack: with its Rejuvenation rolling there is nothing to add,
    /// and the damage goes out.
    #[test]
    fn a_dying_teammate_in_reach_is_healed_before_moonfire() {
        let attacked = |mage_attacked: bool| {
            let (mut s, kill) = kill_scene(0.7, 0.47);
            let (me, warrior, mage) = (s.units[0], s.units[1], s.units[2]);
            mark_everyone(&mut s);
            all_heals_rolling(&mut s, warrior);
            s.auras.entry(mage).or_default().extend(rejuvenation(me));
            for dz in [-3.0, 3.0] {
                add_enemy(
                    &mut s,
                    Mage,
                    BEHIND + Vec3::new(10.0, 0.0, dz),
                    Some(warrior),
                );
            }
            if mage_attacked {
                add_enemy(
                    &mut s,
                    Mage,
                    IN_SIGHT + Vec3::new(-10.0, 0.0, 0.0),
                    Some(mage),
                );
            }
            (s, kill, mage)
        };

        let (mut s, kill, mage) = attacked(true);
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            chose("Lifebloom", mage),
            "a Mage at 47% carrying Rejuvenation, under attack"
        );

        let (mut s, kill, _) = attacked(false);
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            chose("Moonfire", kill),
            "a Mage at 47% carrying Rejuvenation, attacked by nobody"
        );

        let (mut s, kill) = kill_scene(1.0, 1.0);
        mark_everyone(&mut s);
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            chose("Moonfire", kill),
            "nobody hurt"
        );
    }

    /// A dying teammate with no melee threat on it gets step 7's heal — a
    /// Rejuvenation the governor would refuse anyone else — and it is the one
    /// in SIGHT: a lower dying teammate behind the pillar is passed over.
    #[test]
    fn step_seven_heals_the_lowest_dying_teammate_in_sight() {
        let (mut s, kill) = kill_scene(0.46, 0.48);
        let (warrior, mage) = (s.units[1], s.units[2]);
        mark_everyone(&mut s);
        s.prep = Some(under_the_reserve);
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            chose("Rejuvenation", warrior),
            "no pillar: the lowest dying teammate"
        );
        assert_eq!(
            outcome(&take(&mut s, &pillar(), kill, None)),
            chose("Rejuvenation", mage),
            "the Warrior behind the pillar yields to the dying Mage in sight"
        );
    }

    /// `urgency_hp_threshold` is a strict bound: a teammate AT it is not dying.
    /// The Mage there is the focus (an enemy caster is on it), so a Mark held
    /// back for a dying teammate would yield to the focus Rejuvenation.
    #[test]
    fn a_teammate_at_the_urgency_threshold_is_not_dying() {
        let threshold = MovementConfig::default().shared.urgency_hp_threshold;

        let (mut s, kill) = kill_scene(1.0, threshold);
        let (me, mage) = (s.units[0], s.units[2]);
        add_enemy(
            &mut s,
            Mage,
            IN_SIGHT + Vec3::new(-10.0, 0.0, 0.0),
            Some(mage),
        );
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            chose("MarkOfTheWild", me),
            "the Mark leads"
        );

        let (mut s, kill) = kill_scene(1.0, threshold);
        mark_everyone(&mut s);
        s.prep = Some(under_the_reserve);
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            None,
            "no step 7 heal at the threshold"
        );

        let (mut s, kill) = kill_scene(1.0, threshold - 0.01);
        let mage = s.units[2];
        mark_everyone(&mut s);
        s.prep = Some(under_the_reserve);
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            chose("Rejuvenation", mage),
            "just below it"
        );
    }

    /// A melee closing on a dying teammate is rooted — a PEEL, ahead of any
    /// Lifebloom stack and never held for the medic walk. The Mage carries the
    /// Druid's Rejuvenation; an enemy Warrior targets it from 12yd, closing,
    /// not yet attacking.
    #[test]
    fn a_melee_closing_on_a_dying_teammate_is_rooted() {
        let closing = || {
            let (mut s, kill) = kill_scene(0.4, 0.47);
            let (me, mage) = (s.units[0], s.units[2]);
            mark_everyone(&mut s);
            s.auras.entry(mage).or_default().extend(rejuvenation(me));
            let enemy = add_enemy(
                &mut s,
                Warrior,
                IN_SIGHT + Vec3::new(0.0, 0.0, 12.0),
                Some(mage),
            );
            (s, kill, enemy)
        };

        let (mut s, kill, enemy) = closing();
        // The Warrior ally is in sight below the emergency threshold: give it
        // its heals so step 2-3 has nothing to do.
        let warrior = s.units[1];
        all_heals_rolling(&mut s, warrior);
        s.prep = Some(|c| {
            c.ability_cooldowns.insert(AbilityType::Swiftmend, 10.0);
        });
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            chose("EntanglingRoots", enemy),
            "a Warrior closing on the dying Mage"
        );

        // The medic walk to the Warrior behind the pillar holds damage and
        // utility; the peel goes anyway.
        let (mut s, kill, enemy) = closing();
        let warrior = s.units[1];
        assert_eq!(
            outcome(&take(&mut s, &pillar(), kill, Some(warrior))),
            chose("EntanglingRoots", enemy),
            "the peel under the medic walk's hold"
        );
    }

    /// The Cyclone peel comes before the Roots peel: a Warrior beating on the
    /// dying Mage (the focus, every heal rolling) is cycloned, not rooted.
    #[test]
    fn the_cyclone_peel_comes_before_the_roots_peel() {
        let (mut s, kill) = kill_scene(1.0, 0.47);
        let mage = s.units[2];
        mark_everyone(&mut s);
        all_heals_rolling(&mut s, mage);
        let attacker = add_enemy(
            &mut s,
            Warrior,
            IN_SIGHT + Vec3::new(1.0, 0.0, 1.0),
            Some(mage),
        );
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            chose("Cyclone", attacker),
            "a Warrior attacking the dying Mage"
        );
    }

    /// While the medic walk is live and its teammate is behind the pillar, the
    /// Druid holds Moonfire: the global cooldown it would spend is the one in
    /// which sight comes back. Once the teammate is in reach it is healed.
    #[test]
    fn the_medic_walk_holds_damage_until_its_teammate_is_reached() {
        let (mut s, kill) = kill_scene(0.4, 1.0);
        let warrior = s.units[1];
        mark_everyone(&mut s);

        let trace = take(&mut s, &pillar(), kill, Some(warrior));
        assert_eq!(outcome(&trace), None, "the Warrior is behind the pillar");
        let note = &candidate(&trace, "Moonfire")["reason"]["PreconditionUnmet"]["note"];
        assert!(
            note.as_str().is_some_and(|n| n.contains("medic walk")),
            "Moonfire is held for the walk: {note}"
        );

        // Sight back: the dying Warrior gets the heal.
        assert_eq!(
            outcome(&take(&mut s, &[], kill, Some(warrior))),
            chose("Rejuvenation", warrior),
            "the Warrior in reach"
        );

        // The hold is the walk's: with no walk live, the Druid with nothing it
        // can heal Moonfires as before.
        assert_eq!(
            outcome(&take(&mut s, &pillar(), kill, None)),
            chose("Moonfire", kill),
            "no medic walk"
        );
    }

    /// The hold lets go once the walk's teammate is in reach: with every heal
    /// already rolling on it there is nothing to add, and Moonfire goes out
    /// while the walk is still live.
    #[test]
    fn the_hold_releases_once_the_walks_teammate_is_in_reach() {
        let (mut s, kill) = kill_scene(1.0, 0.47);
        let mage = s.units[2];
        mark_everyone(&mut s);
        all_heals_rolling(&mut s, mage);
        assert_eq!(
            outcome(&take(&mut s, &[], kill, Some(mage))),
            chose("Moonfire", kill),
            "the walk's Mage in sight with every heal rolling"
        );
    }

    /// The Cyclone peel is never held: the walk to the dying Warrior behind
    /// the pillar holds damage and utility, and a Rogue beating on the focus
    /// (the Mage, below `DRUID_URGENT_HP`, every heal rolling) is cycloned.
    #[test]
    fn the_cyclone_peel_goes_out_under_the_hold() {
        let (mut s, kill) = kill_scene(0.4, 0.55);
        let (warrior, mage) = (s.units[1], s.units[2]);
        mark_everyone(&mut s);
        all_heals_rolling(&mut s, mage);
        let attacker = add_enemy(
            &mut s,
            Rogue,
            IN_SIGHT + Vec3::new(1.0, 0.0, 1.0),
            Some(mage),
        );
        assert_eq!(
            outcome(&take(&mut s, &pillar(), kill, Some(warrior))),
            chose("Cyclone", attacker),
            "the peel for the focus under the medic walk's hold"
        );
    }

    /// Mark of the Wild leads the rotation, but not over a dying teammate's
    /// heal, and not while the medic walk has not reached its teammate.
    #[test]
    fn mark_of_the_wild_waits_behind_a_dying_teammate() {
        let (mut s, kill) = kill_scene(1.0, 0.47);
        let mage = s.units[2];
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            chose("Rejuvenation", mage),
            "the dying Mage's heal before the Mark"
        );

        let (mut s, kill) = kill_scene(1.0, 1.0);
        let me = s.units[0];
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            chose("MarkOfTheWild", me),
            "nobody dying: the Mark leads"
        );

        let (mut s, kill) = kill_scene(0.4, 1.0);
        let warrior = s.units[1];
        let trace = take(&mut s, &pillar(), kill, Some(warrior));
        assert_eq!(
            outcome(&trace),
            None,
            "the walk has not reached the Warrior"
        );
        let note = &candidate(&trace, "MarkOfTheWild")["reason"]["PreconditionUnmet"]["note"];
        assert!(
            note.as_str().is_some_and(|n| n.contains("medic walk")),
            "the Mark is held for the walk: {note}"
        );
    }

    /// The Roots peel picks reach-first: the lowest dying teammate's only
    /// melee is beyond Roots' range, so it yields to the next dying teammate's
    /// melee, which Roots can land on. (Swiftmend and Cyclone on cooldown, the
    /// Druid under the governor's reserve and every heal rolling, so the peel
    /// is the only cast left.)
    #[test]
    fn the_roots_peel_passes_over_an_attacker_beyond_range() {
        let (mut s, kill) = kill_scene(0.40, 0.48);
        let (warrior, mage) = (s.units[1], s.units[2]);
        s.roster.get_mut(&warrior).unwrap().position = Vec3::new(0.0, 1.0, -38.0);
        mark_everyone(&mut s);
        all_heals_rolling(&mut s, warrior);
        all_heals_rolling(&mut s, mage);
        add_enemy(&mut s, Warrior, Vec3::new(0.0, 1.0, -45.0), Some(warrior));
        let near = add_enemy(
            &mut s,
            Rogue,
            IN_SIGHT + Vec3::new(1.0, 0.0, 1.0),
            Some(mage),
        );
        s.prep = Some(|c| {
            under_the_reserve(c);
            c.ability_cooldowns.insert(AbilityType::Swiftmend, 10.0);
            c.ability_cooldowns.insert(AbilityType::Cyclone, 10.0);
        });
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            chose("EntanglingRoots", near)
        );
    }

    /// Step 8's Roots passes over an attacker immune to damage: a Rogue on
    /// the Druid under a damage immunity yields to the Warrior on the focus.
    /// With no other attacker, the immune one is reported.
    #[test]
    fn step_eight_roots_passes_over_an_immune_attacker() {
        let immune = || {
            vec![Aura {
                effect_type: AuraType::DamageImmunity,
                ..Default::default()
            }]
        };
        let (mut s, kill) = kill_scene(1.0, 0.7);
        let (me, mage) = (s.units[0], s.units[2]);
        mark_everyone(&mut s);
        all_heals_rolling(&mut s, mage);
        let rogue = add_enemy(&mut s, Rogue, Vec3::new(3.0, 1.0, 0.0), Some(me));
        s.auras.insert(rogue, immune());
        let warrior = add_enemy(
            &mut s,
            Warrior,
            IN_SIGHT + Vec3::new(1.0, 0.0, 1.0),
            Some(mage),
        );
        assert_eq!(
            outcome(&take(&mut s, &[], kill, None)),
            chose("EntanglingRoots", warrior),
            "the Warrior on the focus, past the immune Rogue on the Druid"
        );

        let (mut s, kill) = kill_scene(1.0, 1.0);
        let me = s.units[0];
        mark_everyone(&mut s);
        // The Rogue makes the Druid the focus: its heals are rolling.
        all_heals_rolling(&mut s, me);
        let rogue = add_enemy(&mut s, Rogue, Vec3::new(3.0, 1.0, 0.0), Some(me));
        s.auras.insert(rogue, immune());
        let trace = take(&mut s, &[], kill, None);
        assert_eq!(
            candidate(&trace, "EntanglingRoots")["reason"],
            "TargetImmune",
            "only the immune Rogue"
        );
    }

    /// The Cyclone peel waits for the focus to drop below `DRUID_URGENT_HP`:
    /// above it, the Rogue attacking the focus is rooted, not cycloned.
    #[test]
    fn the_cyclone_peel_waits_for_the_urgent_threshold() {
        let cast_on_the_attacker = |mage_hp: f32| {
            let (mut s, kill) = kill_scene(1.0, mage_hp);
            let mage = s.units[2];
            mark_everyone(&mut s);
            all_heals_rolling(&mut s, mage);
            let rogue = add_enemy(
                &mut s,
                Rogue,
                IN_SIGHT + Vec3::new(1.0, 0.0, 1.0),
                Some(mage),
            );
            (outcome(&take(&mut s, &[], kill, None)), rogue)
        };
        let (above, rogue) = cast_on_the_attacker(DRUID_URGENT_HP + 0.05);
        assert_eq!(
            above,
            chose("EntanglingRoots", rogue),
            "the focus above the threshold"
        );
        let (below, rogue) = cast_on_the_attacker(DRUID_URGENT_HP - 0.05);
        assert_eq!(below, chose("Cyclone", rogue), "the focus below it");
    }

    /// A Druid out of mana for Roots is refused it once a frame: the peel
    /// leaves the refusal to step 8.
    #[test]
    fn a_druid_out_of_mana_traces_one_roots_refusal() {
        let (mut s, kill) = kill_scene(1.0, 0.47);
        let mage = s.units[2];
        mark_everyone(&mut s);
        all_heals_rolling(&mut s, mage);
        add_enemy(
            &mut s,
            Warrior,
            IN_SIGHT + Vec3::new(0.0, 0.0, 12.0),
            Some(mage),
        );
        s.prep = Some(|c| c.current_mana = 0.0);
        let trace = take(&mut s, &[], kill, None);
        let event = serde_json::to_value(&trace.pending_events[0]).unwrap();
        let roots: Vec<_> = event["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["ability"] == "EntanglingRoots")
            .collect();
        assert_eq!(roots.len(), 1, "{event}");
        assert!(
            roots[0]["reason"].get("InsufficientMana").is_some(),
            "{event}"
        );
    }
}
