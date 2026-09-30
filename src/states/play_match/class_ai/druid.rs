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
//! 7. Rejuvenation on any other injured ally.
//! 8. Moonfire on the kill target.
//!
//! Steps 5-8 answer to the MANA GOVERNOR ([`mana_reserve`]): proactive heals
//! and damage are paid for only out of mana above a reserve that shrinks as
//! the match heads into dampening. The emergency steps (2, 3) never ask it,
//! and neither does a focused ally who has dropped below [`DRUID_URGENT_HP`].
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
/// to entity order. `None` when nobody is under threat.
pub fn focused_ally<'c>(
    ctx: &'c CombatContext,
    threat_radius: f32,
) -> Option<(&'c CombatantInfo, AllyThreat)> {
    ctx.alive_allies()
        .into_iter()
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
    gates_opened: bool,
    time_since_gates: f32,
    decision_trace: &mut DecisionTrace,
) -> bool {
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
    time_since_gates: f32,
    builder: &'a mut DecisionEventBuilder<'b>,
}

impl DruidTurn<'_, '_, '_, '_, '_> {
    /// Steps 2-8 — everything after the pre-match buff. Every heal is an
    /// instant, so an ESCAPE window has nothing to defer: the Druid heals on
    /// the run.
    fn rotation(&mut self, combatant: &mut Combatant) -> bool {
        // 2-3. The emergency button, and the Rejuvenation that arms it.
        let dying = self
            .ctx
            .lowest_health_ally_below(DRUID_EMERGENCY_HP, self.heal_range, self.my_pos)
            .map(|a| (a.entity, a.position));
        if let Some((ally, ally_pos)) = dying {
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
        } else {
            self.builder
                .reject(AbilityType::Swiftmend, RejectionReason::NoValidTarget);
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

        // 5-6. Keep the focused ally's heals rolling.
        let focus = focused_ally(self.ctx, self.threat_radius)
            .filter(|(a, _)| self.my_pos.distance(a.position) <= self.heal_range)
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

        // 7. Rejuvenation on anyone else who is hurt and has none.
        let focus_entity = focus.map(|(e, _, _, _)| e);
        let top_up = self
            .ctx
            .alive_allies()
            .into_iter()
            .filter(|a| {
                a.health_pct() < DRUID_TOP_UP_HP
                    && self.my_pos.distance(a.position) <= self.heal_range
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

        // 8. Moonfire on the kill target, when it is not already burning.
        self.try_moonfire(combatant)
    }

    /// Mark of the Wild on the first ally (self included, pets excluded) who
    /// does not carry it. Applied directly, like Power Word: Fortitude: a
    /// pre-match buff has no cast to resolve.
    fn try_mark_of_the_wild(&mut self, combatant: &mut Combatant) -> bool {
        let ability = AbilityType::MarkOfTheWild;
        let unbuffed = self
            .ctx
            .alive_allies()
            .into_iter()
            .find(|a| {
                !self.ctx.active_auras.get(&a.entity).is_some_and(|auras| {
                    auras
                        .iter()
                        .any(|aura| aura.compound == Some(CompoundDebuff::MarkOfTheWild))
                })
            })
            .map(|a| (a.entity, a.position));
        let Some((target, target_pos)) = unbuffed else {
            self.builder
                .reject(ability, RejectionReason::AlreadyApplied);
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
            combatant.ability_cooldowns.insert(ability, def.cooldown);
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
        let opts = if ability == AbilityType::Moonfire {
            PreCastOpts {
                check_friendly_cc: true,
                check_target_immune: true,
                ..Default::default()
            }
        } else {
            PreCastOpts::default()
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
