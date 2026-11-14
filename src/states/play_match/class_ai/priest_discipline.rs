//! The Priest's Discipline talent abilities (AS-414): Inner Focus, Power
//! Infusion and Pain Suppression.
//!
//! Each is a `talent: true` ability, in a Priest's book only when its build
//! takes the node that grants it, so each rule first asks the book and does
//! nothing at all without it: an untalented Priest decides exactly as it did
//! before these existed (`tests/talent_ai_audit.rs` holds every rule to both
//! halves). Every rule reads its situation through `ctx.situation()`:
//!
//! - **Pain Suppression** on the teammate in danger (`ally_in_danger`), the
//!   Priest itself included.
//! - **Power Infusion** on a spell-power damage dealer of the team (a
//!   non-healer whose damage scales with spell power: a Mage, a Warlock, a
//!   Shadowform Priest) while the team has a burst moment — a kill window or
//!   the enemy healer locked. A team with no such damage dealer infuses the
//!   Priest's own heals instead, while a teammate is dying.
//! - **Inner Focus** just ahead of a Flash Heal on a dying teammate: the heal
//!   that follows is free and likelier to crit.
//!
//! Inner Focus and Power Infusion cost no global cooldown, as in TBC
//! (`SpellCooldowns.StartRecoveryTime` 0 for 14751 and 10060); Pain
//! Suppression takes one (33206: 1500). The decisions are pure
//! ([`pain_suppression_target`], [`power_infusion_target`],
//! [`inner_focus_due`]) and the casts are `try_*` steps the Priest's chains
//! run (`priest::decide_priest_action`, `priest::decide_shadow_priest_action`).
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::abilities::ScalingStat;
use crate::states::play_match::ability_book::AbilityBook;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::GCD;
use crate::states::play_match::decision_trace::{DecisionEventBuilder, RejectionReason};
use crate::states::play_match::situation::TeamSituation;
use crate::states::play_match::utils::log_ability_use;

use super::cast_guard::{cast_gate, own_state_ok, unreached_reason, PreCastOpts};
use super::{cast_reach, CastReach, CombatContext, CombatantInfo};

/// The teammate Pain Suppression goes on: the teammate in danger, the Priest
/// `_me` itself included (TBC casts it on any friendly target).
pub fn pain_suppression_target(situation: &TeamSituation, _me: Entity) -> Option<Entity> {
    situation.ally_in_danger.map(|w| w.subject)
}

/// Whether `info` is a damage dealer Power Infusion feeds: a non-pet,
/// non-healer whose own book holds a damage ability that scales with spell
/// power. A Hunter, a Rogue or a Warrior is not one; a Shadowform Priest is.
pub fn spell_power_damage_dealer(info: &CombatantInfo) -> bool {
    !info.is_pet
        && !info.is_healer()
        && info.book.iter().any(|(_, config)| {
            config.damage_scales_with == ScalingStat::SpellPower && config.damage_coefficient > 0.0
        })
}

/// Who Power Infusion goes on, among the living teammates (`me`, the Priest,
/// included) that `reaches` accepts:
///
/// - while the team has a spell-power damage dealer (`feeds`), the first of
///   them in entity order, and only in a burst moment — a kill window open or
///   the enemy healer locked;
/// - otherwise the Priest itself, while a teammate is dying: the infusion
///   feeds its heals.
///
/// `None` when neither holds.
pub fn power_infusion_target(
    situation: &TeamSituation,
    me: Entity,
    feeds: impl Fn(Entity) -> bool,
    reaches: impl Fn(Entity) -> bool,
) -> Option<Entity> {
    let dealers: Vec<Entity> = situation
        .allies
        .iter()
        .map(|a| a.entity)
        .filter(|e| feeds(*e))
        .collect();
    if !dealers.is_empty() {
        let burst = situation.kill_window.is_some() || situation.enemy_healer_locked.is_some();
        if !burst {
            return None;
        }
        return dealers.into_iter().find(|e| reaches(*e));
    }
    situation.dying_allies().next().map(|_| me)
}

/// Whether Inner Focus is due: the Flash Heal it feeds can be cast
/// (`heal_castable`: not locked out of its school, silenced or locked by a
/// choice — its cost does not count, the focus pays it), and the teammate it
/// would go on (`heal_target`, the most injured one it reaches) is dying. A
/// focus raised for a heal that cannot go out would be spent on whatever the
/// Priest casts next.
pub fn inner_focus_due(
    situation: &TeamSituation,
    heal_target: Option<Entity>,
    heal_castable: bool,
) -> bool {
    heal_castable && heal_target.is_some_and(|t| situation.dying_allies().any(|e| e == t))
}

/// Whether `auras` hold an aura of `effect` named `name`.
fn holds(auras: Option<&[Aura]>, effect: AuraType, name: &str) -> bool {
    auras.is_some_and(|list| {
        list.iter()
            .any(|a| a.effect_type == effect && a.ability_name == name)
    })
}

/// Pain Suppression: on the teammate in danger.
pub fn try_pain_suppression(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::PainSuppression;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    let Some((subject, info)) = pain_suppression_target(ctx.situation(), entity)
        .and_then(|s| ctx.combatants.get(&s).map(|info| (s, info)))
    else {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "no teammate in danger".into(),
            },
        );
        return false;
    };
    let theirs = ctx.active_auras.get(&subject).map(Vec::as_slice);
    let immune = theirs.is_some_and(|list| {
        list.iter()
            .any(|a| a.effect_type == AuraType::DamageImmunity)
    });
    if immune || holds(theirs, AuraType::DamageTakenReduction, &def.name) {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }
    let target_arg = (subject != entity).then_some((subject, info.position));
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        target_arg,
        ctx,
        PreCastOpts::default(),
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(subject), true);
    combatant.current_mana -= def.mana_cost;
    combatant.start_cooldown(ability);
    combatant.global_cooldown = GCD;
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        Some(info.log_id()),
        "casts",
    );
    if let Some(pending) = AuraPending::from_ability(subject, entity, def) {
        commands.spawn(pending);
    }
    true
}

/// Power Infusion: on a spell-power damage dealer in a burst moment, or on
/// the Priest's own heals while a teammate is dying. No global cooldown.
pub fn try_power_infusion(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::PowerInfusion;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    let feeds = |e: Entity| {
        ctx.combatants
            .get(&e)
            .is_some_and(spell_power_damage_dealer)
    };
    let reach = |e: Entity| {
        ctx.combatants.get(&e).map(|info| {
            if e == entity {
                CastReach::Reaches
            } else {
                cast_reach(ctx, def.range, my_pos, info.position)
            }
        })
    };
    let infused = |e: Entity| {
        holds(
            ctx.active_auras.get(&e).map(Vec::as_slice),
            AuraType::SpellPowerIncrease,
            &def.name,
        )
    };
    let situation = ctx.situation();
    let Some(target) = power_infusion_target(situation, entity, feeds, |e| {
        reach(e) == Some(CastReach::Reaches) && !infused(e)
    }) else {
        // A damage dealer the burst would infuse but cannot be reached is
        // reported as such; anything else is no moment for it.
        let burst = situation.kill_window.is_some() || situation.enemy_healer_locked.is_some();
        let unreached = situation
            .allies
            .iter()
            .filter(|a| burst && feeds(a.entity) && !infused(a.entity))
            .find_map(|a| match reach(a.entity) {
                Some(CastReach::LosBlocked) => Some(RejectionReason::LosBlocked),
                Some(CastReach::OutOfRange { distance }) => Some(RejectionReason::OutOfRange {
                    distance,
                    max: def.range,
                }),
                _ => None,
            });
        let reason = match unreached {
            Some(reason) => unreached_reason(
                reason,
                ability,
                def,
                combatant,
                my_pos,
                auras,
                ctx,
                PreCastOpts::default(),
            ),
            None if situation
                .allies
                .iter()
                .any(|a| feeds(a.entity) && infused(a.entity)) =>
            {
                RejectionReason::AlreadyApplied
            }
            None => RejectionReason::PreconditionUnmet {
                note: "no burst moment for a spell-power damage dealer, and no teammate dying"
                    .into(),
            },
        };
        builder.reject(ability, reason);
        return false;
    };
    let Some(info) = ctx.combatants.get(&target) else {
        return false;
    };
    let target_arg = (target != entity).then_some((target, info.position));
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        target_arg,
        ctx,
        PreCastOpts::default(),
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };

    builder.choose(cleared, Some(target), true);
    combatant.current_mana -= def.mana_cost;
    combatant.start_cooldown(ability);
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        Some(info.log_id()),
        "casts",
    );
    if let Some(pending) = AuraPending::from_ability(target, entity, def) {
        commands.spawn(pending);
    }
    true
}

/// Inner Focus: just ahead of a Flash Heal on a dying teammate. No mana, no
/// global cooldown; the heal goes out on the next frame, free and likelier
/// to crit.
pub fn try_inner_focus(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    heal_hp: f32,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::InnerFocus;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    if holds(
        auras.map(|a| a.auras.as_slice()),
        AuraType::NextCast,
        &def.name,
    ) {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        None,
        ctx,
        PreCastOpts::default(),
    ) {
        Ok(cleared) => cleared,
        Err(reason) => {
            builder.reject(ability, reason);
            return false;
        }
    };
    let heal = abilities.get(&AbilityType::FlashHeal);
    let heal_target = heal.and_then(|heal| {
        ctx.lowest_health_ally_in_reach(heal_hp, heal.range, my_pos)
            .ok()
            .map(|info| info.entity)
    });
    let heal_castable = heal.is_some_and(|heal| {
        own_state_ok(
            AbilityType::FlashHeal,
            heal,
            combatant,
            auras,
            PreCastOpts::default(),
        )
    });
    if !inner_focus_due(ctx.situation(), heal_target, heal_castable) {
        let note = if heal_castable {
            "no dying teammate for the Flash Heal it would feed"
        } else {
            "the Flash Heal it would feed cannot be cast"
        };
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet { note: note.into() },
        );
        return false;
    }

    builder.choose(cleared, Some(entity), true);
    combatant.start_cooldown(ability);
    log_ability_use(
        combat_log,
        combatant.team,
        combatant.slot,
        combatant.class,
        &def.name,
        None,
        "casts",
    );
    if let Some(pending) = AuraPending::from_ability(entity, entity, def) {
        commands.spawn(pending);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::situation::{AllyStanding, Window};

    fn standing(entity: Entity, dying: bool) -> AllyStanding {
        AllyStanding {
            entity,
            dying,
            pressured: dying,
            pressure_onset: dying,
            held_attacker: false,
        }
    }

    fn window(subject: Entity) -> Window {
        Window {
            subject,
            since: 0.0,
            until: None,
        }
    }

    #[test]
    fn pain_suppression_goes_on_the_teammate_in_danger_the_priest_included() {
        let me = Entity::from_raw(1);
        let ally = Entity::from_raw(2);
        let mut s = TeamSituation::default();
        assert_eq!(pain_suppression_target(&s, me), None);
        s.ally_in_danger = Some(window(ally));
        assert_eq!(pain_suppression_target(&s, me), Some(ally));
        s.ally_in_danger = Some(window(me));
        assert_eq!(pain_suppression_target(&s, me), Some(me));
    }

    #[test]
    fn power_infusion_feeds_a_damage_dealer_in_a_burst_moment() {
        let me = Entity::from_raw(1);
        let mage = Entity::from_raw(2);
        let mut s = TeamSituation::default();
        s.allies.push(standing(me, false));
        s.allies.push(standing(mage, false));
        let feeds = |e: Entity| e == mage;
        assert_eq!(power_infusion_target(&s, me, feeds, |_| true), None);
        s.kill_window = Some(window(Entity::from_raw(9)));
        assert_eq!(power_infusion_target(&s, me, feeds, |_| true), Some(mage));
        assert_eq!(power_infusion_target(&s, me, feeds, |e| e != mage), None);
        s.kill_window = None;
        s.enemy_healer_locked = Some(window(Entity::from_raw(9)));
        assert_eq!(power_infusion_target(&s, me, feeds, |_| true), Some(mage));
        // A dying teammate does not turn a team with a dealer to the heals.
        s.enemy_healer_locked = None;
        s.allies[1].dying = true;
        assert_eq!(power_infusion_target(&s, me, feeds, |_| true), None);
    }

    #[test]
    fn power_infusion_feeds_the_priests_heals_on_a_team_without_a_dealer() {
        let me = Entity::from_raw(1);
        let warrior = Entity::from_raw(2);
        let mut s = TeamSituation::default();
        s.allies.push(standing(me, false));
        s.allies.push(standing(warrior, false));
        let none = |_: Entity| false;
        assert_eq!(power_infusion_target(&s, me, none, |_| true), None);
        s.kill_window = Some(window(Entity::from_raw(9)));
        assert_eq!(power_infusion_target(&s, me, none, |_| true), None);
        s.allies[1].dying = true;
        assert_eq!(power_infusion_target(&s, me, none, |_| true), Some(me));
    }

    #[test]
    fn inner_focus_is_due_only_for_a_heal_on_a_dying_teammate() {
        let ally = Entity::from_raw(2);
        let mut s = TeamSituation::default();
        s.allies.push(standing(ally, false));
        assert!(!inner_focus_due(&s, None, true));
        assert!(!inner_focus_due(&s, Some(ally), true));
        s.allies[0].dying = true;
        assert!(inner_focus_due(&s, Some(ally), true));
        assert!(!inner_focus_due(&s, None, true));
        // A heal that cannot be cast is fed nothing.
        assert!(!inner_focus_due(&s, Some(ally), false));
    }
}
