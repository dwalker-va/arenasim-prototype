//! The Paladin's Protection talent abilities (AS-421): Divine Protection,
//! Blessing of Sacrifice and Sacred Shield.
//!
//! Each is a `talent: true` ability, in a Paladin's book only when its build
//! takes the node that grants it, so each rule first asks the book and does
//! nothing at all without it: an untalented Paladin decides exactly as it did
//! before these existed (`tests/talent_ai_audit.rs` holds every rule to both
//! halves). Every rule reads its situation through `ctx.situation()`:
//!
//! - **Divine Protection** when the Paladin itself is in danger: dying and
//!   pressured (the situation's `in_danger`).
//! - **Blessing of Sacrifice** on the teammate in danger
//!   (`ally_in_danger`), never the Paladin itself.
//! - **Sacred Shield** kept on the teammate most likely to be focused: the
//!   teammate in danger, else the most injured pressured one, among those it
//!   reaches. Recast only once it has fallen off.
//!
//! The decisions are pure ([`divine_protection_due`], [`sacrifice_target`],
//! [`sacred_shield_pick`]) and the casts are `try_*` steps the Paladin's
//! chain runs (`paladin::decide_paladin_action`).
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_book::AbilityBook;
use crate::states::play_match::components::*;
use crate::states::play_match::constants::GCD;
use crate::states::play_match::decision_trace::{DecisionEventBuilder, RejectionReason};
use crate::states::play_match::situation::TeamSituation;
use crate::states::play_match::utils::log_ability_use;

use super::cast_guard::{cast_gate, unreached_reason, PreCastOpts};
use super::{cast_reach, CastReach, CombatContext};

/// Whether the Paladin `me` should raise Divine Protection: it is dying and
/// pressured right now.
pub fn divine_protection_due(situation: &TeamSituation, me: Entity) -> bool {
    situation.in_danger.iter().any(|(e, _)| *e == me)
}

/// The teammate Blessing of Sacrifice goes on: the teammate in danger, when it
/// is not the Paladin `me` (the blessing moves damage TO the Paladin).
pub fn sacrifice_target(situation: &TeamSituation, me: Entity) -> Option<Entity> {
    situation
        .ally_in_danger
        .map(|w| w.subject)
        .filter(|subject| *subject != me)
}

/// The teammate Sacred Shield belongs on, among those `reaches` accepts: the
/// teammate in danger; else `holder`, the teammate already holding this
/// Paladin's shield, while it is still pressured (the shield is one target a
/// Paladin, so moving it between two pressured teammates as their health
/// trades places would spend a global cooldown each time and re-arm the
/// absorb); else the most injured pressured teammate (by `health`, a
/// fraction; entity order on a tie). `None` while no teammate it reaches is
/// pressured.
pub fn sacred_shield_pick(
    situation: &TeamSituation,
    health: impl Fn(Entity) -> f32,
    reaches: impl Fn(Entity) -> bool,
    holder: Option<Entity>,
) -> Option<Entity> {
    if let Some(subject) = situation.ally_in_danger.map(|w| w.subject) {
        if reaches(subject) {
            return Some(subject);
        }
    }
    if let Some(holder) = holder {
        if situation.is_pressured(holder) && reaches(holder) {
            return Some(holder);
        }
    }
    situation
        .allies
        .iter()
        .filter(|a| a.pressured && reaches(a.entity))
        .map(|a| (a.entity, health(a.entity)))
        .fold(None, |best: Option<(Entity, f32)>, (e, hp)| match best {
            Some((_, b)) if b <= hp => best,
            _ => Some((e, hp)),
        })
        .map(|(e, _)| e)
}

/// Whether `auras` hold an aura of `effect` named `name`.
fn holds(auras: Option<&[Aura]>, effect: AuraType, name: &str) -> bool {
    auras.is_some_and(|list| {
        list.iter()
            .any(|a| a.effect_type == effect && a.ability_name == name)
    })
}

/// Divine Protection: the Paladin is dying and pressured.
pub fn try_divine_protection(
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
    let ability = AbilityType::DivineProtection;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
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
    let own = auras.map(|a| a.auras.as_slice());
    let immune = own.is_some_and(|list| {
        list.iter()
            .any(|a| a.effect_type == AuraType::DamageImmunity)
    });
    if immune || holds(own, AuraType::DamageTakenReduction, &def.name) {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }
    if !divine_protection_due(ctx.situation(), entity) {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "the Paladin is not in danger (dying and pressured)".into(),
            },
        );
        return false;
    }

    builder.choose(cleared, Some(entity), true);
    combatant.current_mana -= def.mana_cost;
    combatant.start_cooldown(ability);
    combatant.global_cooldown = GCD;
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

/// Blessing of Sacrifice: on the teammate in danger.
pub fn try_blessing_of_sacrifice(
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
    let ability = AbilityType::BlessingOfSacrifice;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    let Some((subject, info)) = sacrifice_target(ctx.situation(), entity)
        .and_then(|s| ctx.combatants.get(&s).map(|info| (s, info)))
    else {
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet {
                note: "no other teammate in danger".into(),
            },
        );
        return false;
    };
    let theirs = ctx.active_auras.get(&subject).map(Vec::as_slice);
    if holds(theirs, AuraType::DamageTransfer, &def.name) {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }
    let cleared = match cast_gate(
        ability,
        def,
        combatant,
        my_pos,
        auras,
        Some((subject, info.position)),
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

/// Sacred Shield: kept on the teammate most likely to be focused.
pub fn try_sacred_shield(
    commands: &mut Commands,
    combat_log: &mut CombatLog,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    shielded_this_frame: &mut std::collections::HashSet<Entity>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::SacredShield;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    let situation = ctx.situation();
    let health = |e: Entity| {
        ctx.combatants
            .get(&e)
            .filter(|info| info.max_health > 0.0)
            .map_or(1.0, |info| info.current_health / info.max_health)
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
    // The teammate holding this Paladin's shield (one target a Paladin).
    let holder = ctx
        .active_auras
        .iter()
        .find(|(_, list)| {
            list.iter().any(|a| {
                a.effect_type == AuraType::TriggeredAbsorb
                    && a.ability_name == def.name
                    && a.caster == Some(entity)
            })
        })
        .map(|(e, _)| *e);
    let Some(target) = sacred_shield_pick(
        situation,
        health,
        |e| reach(e) == Some(CastReach::Reaches),
        holder,
    ) else {
        // A pressured teammate it cannot reach is reported as such.
        let unreached = situation
            .allies
            .iter()
            .filter(|a| a.pressured)
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
            None => RejectionReason::PreconditionUnmet {
                note: "no teammate is pressured".into(),
            },
        };
        builder.reject(ability, reason);
        return false;
    };
    let theirs = ctx.active_auras.get(&target).map(Vec::as_slice);
    // Another Paladin's shield put there earlier this frame is not in the
    // snapshot yet: a second cast would only refresh it.
    if holds(theirs, AuraType::TriggeredAbsorb, &def.name) || shielded_this_frame.contains(&target)
    {
        builder.reject(ability, RejectionReason::AlreadyApplied);
        return false;
    }
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
    shielded_this_frame.insert(target);
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
    if let Some(pending) = AuraPending::from_ability(target, entity, def) {
        commands.spawn(pending);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::states::play_match::situation::{AllyStanding, Window};

    fn standing(entity: Entity, pressured: bool) -> AllyStanding {
        AllyStanding {
            entity,
            dying: false,
            pressured,
            pressure_onset: pressured,
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
    fn divine_protection_is_due_only_for_the_paladin_in_danger() {
        let me = Entity::from_raw(1);
        let ally = Entity::from_raw(2);
        let mut s = TeamSituation::default();
        assert!(!divine_protection_due(&s, me));
        s.in_danger.push((ally, 0.0));
        assert!(!divine_protection_due(&s, me));
        s.in_danger.push((me, 0.0));
        assert!(divine_protection_due(&s, me));
    }

    #[test]
    fn sacrifice_goes_on_the_teammate_in_danger_never_the_paladin() {
        let me = Entity::from_raw(1);
        let ally = Entity::from_raw(2);
        let mut s = TeamSituation::default();
        assert_eq!(sacrifice_target(&s, me), None);
        s.ally_in_danger = Some(window(me));
        assert_eq!(sacrifice_target(&s, me), None);
        s.ally_in_danger = Some(window(ally));
        assert_eq!(sacrifice_target(&s, me), Some(ally));
    }

    #[test]
    fn sacred_shield_prefers_the_teammate_in_danger_then_the_most_injured_pressured() {
        let a = Entity::from_raw(1);
        let b = Entity::from_raw(2);
        let c = Entity::from_raw(3);
        let hp = |e: Entity| match e.index() {
            1 => 0.9,
            2 => 0.6,
            _ => 0.3,
        };
        let mut s = TeamSituation::default();
        s.allies.push(standing(a, true));
        s.allies.push(standing(b, true));
        s.allies.push(standing(c, false));
        // The unpressured, most injured teammate is not picked.
        assert_eq!(sacred_shield_pick(&s, hp, |_| true, None), Some(b));
        // Out of reach, the next pressured one is.
        assert_eq!(sacred_shield_pick(&s, hp, |e| e != b, None), Some(a));
        // The teammate in danger first, when it is reached.
        s.ally_in_danger = Some(window(a));
        assert_eq!(sacred_shield_pick(&s, hp, |_| true, None), Some(a));
        assert_eq!(sacred_shield_pick(&s, hp, |e| e != a, None), Some(b));
        // Nobody pressured: no pick.
        let calm = TeamSituation {
            allies: [standing(a, false)].into_iter().collect(),
            ..Default::default()
        };
        assert_eq!(sacred_shield_pick(&calm, hp, |_| true, None), None);
    }

    #[test]
    fn sacred_shield_stays_on_a_holder_while_it_is_pressured() {
        let a = Entity::from_raw(1);
        let b = Entity::from_raw(2);
        let hp = |e: Entity| if e == a { 0.9 } else { 0.6 };
        let mut s = TeamSituation::default();
        s.allies.push(standing(a, true));
        s.allies.push(standing(b, true));
        // The healthier holder keeps it while pressured.
        assert_eq!(sacred_shield_pick(&s, hp, |_| true, Some(a)), Some(a));
        // Unpressured, it moves to the most injured pressured teammate.
        s.allies[0].pressured = false;
        assert_eq!(sacred_shield_pick(&s, hp, |_| true, Some(a)), Some(b));
        // The teammate in danger outranks the holder.
        s.allies[0].pressured = true;
        s.ally_in_danger = Some(window(b));
        assert_eq!(sacred_shield_pick(&s, hp, |_| true, Some(a)), Some(b));
    }
}
