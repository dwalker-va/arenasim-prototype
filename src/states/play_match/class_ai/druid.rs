//! Druid AI Module
//!
//! The Druid is a PROACTIVE heal-over-time healer. The other healers heal on
//! thresholds — wait for an ally to drop, then cast. Every heal the Druid has
//! is an instant, so its AI instead keeps heals rolling on the ally the enemy
//! is attacking BEFORE the damage lands, and paces its mana so it is still
//! healing when arena dampening arrives.
//!
//! The rotation, in priority order:
//! 1. Mark of the Wild on any ally without it (pre-match, in practice).
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
//! 7. Control, when an ally needs a peel or the kill needs its healer gone
//!    (see [`DruidTurn::try_control`]): Cyclone, then Entangling Roots.
//! 8. Rejuvenation on any other injured ally.
//! 9. Moonfire on the kill target.
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
//! Steps 5-9 answer to the MANA GOVERNOR ([`mana_reserve`]): proactive heals,
//! control and damage are paid for only out of mana above a reserve that
//! shrinks as the match heads into dampening. The emergency steps (2, 3), the
//! shift and a peel for a dying ally never ask it, and neither does a focused
//! ally who has dropped below [`DRUID_URGENT_HP`].
//!
//! Movement is the shared caster-healer posture machine
//! (`caster_healer_posture`) on the `druid:` block of `movement.ron`.
#![allow(clippy::too_many_arguments)]

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
use super::cast_guard::{classify_pre_cast_failure, pre_cast_ok, PreCastOpts};
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

/// This Druid's aura from `ability` on `target`, if any.
fn own_aura<'c>(
    ctx: &'c CombatContext,
    abilities: &AbilityDefinitions,
    target: Entity,
    ability: AbilityType,
) -> Option<&'c Aura> {
    let name = &abilities.get_unchecked(&ability).name;
    ctx.active_auras
        .get(&target)
        .and_then(|auras| auras.iter().find(|a| &a.ability_name == name))
}

/// Druid AI: decides and executes one ability per global cooldown.
///
/// `time_since_gates` feeds the mana governor. `gates_opened` holds every
/// combat action until the gates open; only Mark of the Wild is cast before.
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
    gates_opened: bool,
    time_since_gates: f32,
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
            pressured,
            time_since_gates,
            builder: &mut builder,
        };
        turn.try_mark_of_the_wild(combatant) || (gates_opened && turn.rotation(combatant))
    };
    builder.finish();
    acted
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
    /// The posture machine's PRESSURED trigger this tick.
    pressured: bool,
    time_since_gates: f32,
    builder: &'a mut DecisionEventBuilder<'b>,
}

impl DruidTurn<'_, '_, '_, '_, '_> {
    /// Steps 2-8 — everything after the pre-match buff. Every heal is an
    /// instant, so an ESCAPE window has nothing to defer: the Druid heals on
    /// the run.
    fn rotation(&mut self, combatant: &mut Combatant) -> bool {
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

        // 7. Control: a peel for the focused ally, or the enemy healer. The
        // peel's focus is ranked over every ally, sight or not: its attackers
        // may be in sight when the ally is not.
        let control_focus = focused_ally(ctx, self.threat_radius, |_| true)
            .filter(|(a, _)| my_pos.distance(a.position) <= heal_range)
            .map(|(a, _)| (a.entity, a.health_pct()));
        if self.try_control(combatant, control_focus) {
            return true;
        }

        // 8. Rejuvenation on anyone else who is hurt and has none.
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

        // 9. Moonfire on the kill target, when it is not already burning.
        self.try_moonfire(combatant)
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
    /// 3. **Entangling Roots** — a melee enemy attacking the Druid or the
    ///    focused ally (or closing on either) is pinned.
    ///
    /// The kill target is never cycloned or rooted: a Cyclone would make it
    /// immune to the team's damage, and the team's damage would break a root.
    /// A target already under hard crowd control, or immune to the bucket by
    /// diminishing returns, is passed over.
    fn try_control(&mut self, combatant: &mut Combatant, focus: Option<(Entity, f32)>) -> bool {
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

        // 1. Cyclone to peel for a dying focus.
        let peel = focus
            .filter(|&(_, hp)| hp < DRUID_URGENT_HP)
            .and_then(|(ally, _)| {
                attackers_of(self, ally).into_iter().find(|(e, _)| {
                    self.ctx
                        .combatants
                        .get(e)
                        .is_some_and(|info| !info.is_pet && usable(self, info, DRCategory::Cyclone))
                })
            });
        if let Some((enemy, enemy_pos)) = peel {
            if self.cast(combatant, AbilityType::Cyclone, enemy, enemy_pos) {
                return true;
            }
        }

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
            Some((healer, healer_pos)) if kill_close => {
                if self.governed_cast(combatant, AbilityType::Cyclone, healer, healer_pos, false) {
                    return true;
                }
            }
            _ if peel.is_none() => {
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

        // 3. Entangling Roots on a melee on the Druid or on its focus.
        let mut guarded = vec![self.entity];
        if let Some((ally, _)) = focus {
            guarded.push(ally);
        }
        let melee_threat = guarded.into_iter().find_map(|ally| {
            let ally_pos = self.ctx.combatants.get(&ally)?.position;
            self.ctx
                .enemies_targeting(ally)
                .into_iter()
                .filter(|e| {
                    (e.class.is_melee() || e.is_pet)
                        && e.position.distance(ally_pos) <= self.threat_radius
                        && usable(self, e, DRCategory::Roots)
                })
                .min_by(|a, b| {
                    a.position
                        .distance(ally_pos)
                        .partial_cmp(&b.position.distance(ally_pos))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|e| (e.entity, e.position))
        });
        let Some((enemy, enemy_pos)) = melee_threat else {
            self.builder
                .reject(AbilityType::EntanglingRoots, RejectionReason::NoValidTarget);
            return false;
        };
        let urgent = focus.is_some_and(|(_, hp)| hp < DRUID_URGENT_HP);
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
    fn try_mark_of_the_wild(&mut self, combatant: &mut Combatant) -> bool {
        let ability = AbilityType::MarkOfTheWild;
        let mark_range = self.abilities.get_unchecked(&ability).range;
        let mut occluded = false;
        let unbuffed = self
            .ctx
            .alive_allies()
            .into_iter()
            .filter(|a| {
                !self.ctx.is_cycloned(a.entity)
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
            self.builder.reject(
                ability,
                if occluded {
                    RejectionReason::LosBlocked
                } else {
                    RejectionReason::AlreadyApplied
                },
            );
            return false;
        };
        if !self.guard(combatant, ability, target, target_pos) {
            return false;
        }

        let def = self.abilities.get_unchecked(&ability);
        self.builder.choose(ability, Some(target), true);
        combatant.current_mana -= def.mana_cost;
        combatant.global_cooldown = GCD;
        self.log_use(combatant, &def.name, target, "casts");
        if let Some(pending) = AuraPending::from_ability(target, self.entity, def) {
            self.commands.spawn(pending);
        }
        true
    }

    /// Moonfire on the kill target: governed, and skipped while its DoT is
    /// still up or the target is under our own breakable crowd control.
    fn try_moonfire(&mut self, combatant: &mut Combatant) -> bool {
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
        let cost = self.abilities.get_unchecked(&ability).mana_cost;
        if !urgent && !governor_allows(combatant, cost, self.time_since_gates) {
            self.builder.reject(
                ability,
                RejectionReason::PreconditionUnmet {
                    note: "mana governor: holding the reserve for dampening".to_string(),
                },
            );
            return false;
        }
        self.cast(combatant, ability, target, target_pos)
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
        let opts = match ability {
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
        };
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
        own_aura(self.ctx, self.abilities, target, ability)
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
            stacks: Some(AuraStacks { count, max: 3 }),
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

    /// One Druid turn: `step` runs against a [`DruidTurn`] over the scene.
    fn turn(
        s: &mut ReachScene,
        obstacles: &[ObstacleVolume],
        time_since_gates: f32,
        step: impl FnOnce(&mut DruidTurn, &mut Combatant) -> bool,
    ) -> DecisionTrace {
        let abilities = AbilityDefinitions::default();
        let movement = MovementConfig::default();
        let me = s.units[0];
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
                pressured: false,
                time_since_gates,
                builder,
            };
            step(&mut turn, combatant)
        })
    }

    /// One rotation turn (everything after the pre-match buff).
    fn rotation(s: &mut ReachScene, obstacles: &[ObstacleVolume]) -> DecisionTrace {
        turn(s, obstacles, 20.0, |t, c| t.rotation(c))
    }

    fn mark(s: &mut ReachScene, obstacles: &[ObstacleVolume]) -> DecisionTrace {
        turn(s, obstacles, 0.0, |t, c| t.try_mark_of_the_wild(c))
    }

    /// A fresh Rejuvenation, well clear of its refresh window.
    fn rejuvenation() -> Vec<Aura> {
        vec![Aura {
            effect_type: AuraType::HealingOverTime,
            ability_name: "Rejuvenation".to_string(),
            duration: 12.0,
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
        s.auras.insert(mage, rejuvenation());
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
        s.auras.insert(mage, rejuvenation());
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
                s.auras.insert(ally, rejuvenation());
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
}
