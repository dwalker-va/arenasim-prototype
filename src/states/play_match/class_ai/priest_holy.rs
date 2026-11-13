//! The Priest's Holy talent abilities (AS-415): Holy Nova and Guardian
//! Spirit.
//!
//! Each is a `talent: true` ability, in a Priest's book only when its build
//! takes the node that grants it, so each rule first asks the book and does
//! nothing at all without it: an untalented Priest decides exactly as it did
//! before these existed (`tests/talent_ai_audit.rs` holds every rule to both
//! halves).
//!
//! - **Guardian Spirit** on the most endangered teammate in reach, the Priest
//!   itself included ([`guardian_spirit_target`]): one below
//!   [`SPIRIT_FOCUSED_HP`] with [`SPIRIT_FOCUS_ATTACKERS`] or more enemies on
//!   it, or one below [`SPIRIT_ANY_HP`] whatever is on it
//!   ([`spirit_endangered`]). Held off a teammate immune to damage (nothing
//!   can kill it) or hidden by stealth (nothing is hitting it). No global
//!   cooldown, as in Wrath (47788: `StartRecoveryCategory` 0).
//! - **Holy Nova** is the heal a Priest casts when its Flash Heal cannot go
//!   out safely ([`holy_nova_due`]): the teammate the Flash Heal would heal
//!   (the most injured in reach, below the Flash Heal line) stands inside the
//!   nova, and the cast is threatened — an enemy with an interrupt ready
//!   stands within its reach of the Priest ([`interrupt_ready_on`]), or a walk
//!   owns the Priest's legs (an ESCAPE window, a dip, a dispel walk or a medic
//!   walk: the `escape_defer` that defers a Flash Heal that is not critical).
//!   It sits just before Flash Heal, which it replaces: an instant no interrupt
//!   takes and no walk stops. It is held while an enemy inside it is in crowd
//!   control its team holds that any damage breaks (a Polymorph, a Freezing
//!   Trap), since its damage would break it.
//!
//! The decisions are pure ([`spirit_endangered`], [`guardian_spirit_target`],
//! [`holy_nova_due`], [`interrupt_ready_on`]); the casts are `try_*` steps the Priest's chains run
//! (`priest::decide_priest_action`, and Guardian Spirit also in
//! `priest::decide_shadow_priest_action`, as Pain Suppression is).
#![allow(clippy::too_many_arguments)]

use bevy::prelude::*;

use crate::combat::log::CombatLog;
use crate::states::play_match::abilities::AbilityType;
use crate::states::play_match::ability_book::AbilityBook;
use crate::states::play_match::combat_core::{
    get_attack_power_bonus_from_slice, get_crit_chance_bonus_from_slice,
    get_spell_power_bonus_from_slice, roll_crit,
};
use crate::states::play_match::components::*;
use crate::states::play_match::constants::{CRIT_DAMAGE_MULTIPLIER, CRIT_HEALING_MULTIPLIER, GCD};
use crate::states::play_match::decision_trace::{DecisionEventBuilder, RejectionReason};
use crate::states::play_match::utils::log_ability_use;

use super::cast_guard::{cast_gate, unreached_reason, PreCastOpts};
use super::{cast_reach, AreaQueues, CastReach, CombatContext, QueuedAoeDamage, QueuedAoeHeal};

/// Guardian Spirit's focused trigger: a teammate below this health fraction
/// with [`SPIRIT_FOCUS_ATTACKERS`] or more enemies on it.
pub const SPIRIT_FOCUSED_HP: f32 = 0.40;
/// How many enemies on a teammate make it focused.
pub const SPIRIT_FOCUS_ATTACKERS: usize = 2;
/// Guardian Spirit's trigger whatever is on the teammate: below this health
/// fraction.
pub const SPIRIT_ANY_HP: f32 = 0.25;

/// Whether a teammate at `health_pct` with `attackers` enemies on it is in
/// danger enough for Guardian Spirit: below [`SPIRIT_FOCUSED_HP`] and
/// focused by [`SPIRIT_FOCUS_ATTACKERS`] or more, or below [`SPIRIT_ANY_HP`]
/// from any source.
pub fn spirit_endangered(health_pct: f32, attackers: usize) -> bool {
    health_pct < SPIRIT_ANY_HP
        || (health_pct < SPIRIT_FOCUSED_HP && attackers >= SPIRIT_FOCUS_ATTACKERS)
}

/// One teammate as Guardian Spirit weighs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpiritCandidate {
    pub entity: Entity,
    pub health_pct: f32,
    /// Enemies whose target it is.
    pub attackers: usize,
    /// Immune to damage (Divine Shield, a Cyclone): nothing can kill it.
    pub immune: bool,
    /// Hidden by stealth: nothing is hitting it.
    pub stealthed: bool,
    /// Already holding a Guardian Spirit.
    pub spirited: bool,
}

/// The teammate Guardian Spirit goes on: the most endangered (lowest health
/// fraction) of the `candidates` [`spirit_endangered`] names, passing over one
/// that is immune, stealthed or already spirited. `None` when no one
/// qualifies. Ties go to the first in `candidates`' order (entity order).
pub fn guardian_spirit_target(candidates: &[SpiritCandidate]) -> Option<Entity> {
    candidates
        .iter()
        .filter(|c| {
            spirit_endangered(c.health_pct, c.attackers) && !c.immune && !c.stealthed && !c.spirited
        })
        .min_by(|a, b| a.health_pct.total_cmp(&b.health_pct))
        .map(|c| c.entity)
}

/// Whether Holy Nova is due: the Flash Heal's target stands inside the nova
/// (`target_inside`), that Flash Heal is threatened (`threatened`: an enemy's
/// interrupt is ready for it, or a walk would defer it), and no enemy inside
/// the nova is held in crowd control its damage would break
/// (`breaks_team_cc`).
pub fn holy_nova_due(target_inside: bool, threatened: bool, breaks_team_cc: bool) -> bool {
    target_inside && threatened && !breaks_team_cc
}

/// Whether an enemy of `team` could interrupt a cast begun at `pos` now: a
/// visible living one, not held in crowd control that stops it acting, with
/// an interrupt in its own book (`is_interrupt`: Kick, Pummel, Wind Shear, a
/// Felhunter's Spell Lock) off cooldown, affordable (a Warrior's rage, a
/// Rogue's energy) and within that interrupt's range.
pub fn interrupt_ready_on(ctx: &CombatContext, team: u8, pos: Vec3) -> bool {
    ctx.combatants.values().any(|enemy| {
        enemy.team != team
            && enemy.is_alive
            && !ctx
                .active_auras
                .get(&enemy.entity)
                .is_some_and(|auras| auras.iter().any(|a| super::holds(a.effect_type)))
            && enemy.book.iter().any(|(ability, def)| {
                def.is_interrupt
                    && enemy.position.distance(pos) <= def.range
                    && enemy.current_mana >= def.mana_cost
                    && ctx
                        .ability_cooldowns
                        .get(&enemy.entity)
                        .and_then(|cds| cds.get(ability))
                        .is_none_or(|left| *left <= 0.0)
            })
    })
}

/// Whether `auras` hold a Guardian Spirit (its face, by the compound it
/// belongs to).
fn spirited(auras: Option<&Vec<Aura>>) -> bool {
    auras.is_some_and(|list| {
        list.iter()
            .any(|a| a.compound == Some(CompoundDebuff::GuardianSpirit))
    })
}

/// Guardian Spirit: on the most endangered teammate in reach. No global
/// cooldown.
pub fn try_guardian_spirit(
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
    let ability = AbilityType::GuardianSpirit;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    let my_team = combatant.team;
    let reach = |info: &super::CombatantInfo| {
        if info.entity == entity {
            CastReach::Reaches
        } else {
            cast_reach(ctx, def.range, my_pos, info.position)
        }
    };
    let allies: Vec<&super::CombatantInfo> = ctx
        .combatants
        .values()
        .filter(|c| c.team == my_team && c.is_alive && !c.is_pet)
        .collect();
    let candidate = |info: &super::CombatantInfo| SpiritCandidate {
        entity: info.entity,
        health_pct: info.health_pct(),
        attackers: ctx
            .combatants
            .values()
            .filter(|c| c.team != my_team && c.is_alive && c.target == Some(info.entity))
            .count(),
        immune: ctx.entity_is_immune(info.entity),
        stealthed: info.stealthed,
        spirited: spirited(ctx.active_auras.get(&info.entity)),
    };
    let in_reach: Vec<SpiritCandidate> = allies
        .iter()
        .filter(|info| reach(info) == CastReach::Reaches)
        .map(|info| candidate(info))
        .collect();
    let Some(target) = guardian_spirit_target(&in_reach) else {
        // A teammate the spirit would go on but cannot reach is reported as
        // such; anything else is no moment for it.
        let all: Vec<SpiritCandidate> = allies.iter().map(|info| candidate(info)).collect();
        let reason = match guardian_spirit_target(&all)
            .and_then(|e| allies.iter().find(|info| info.entity == e))
            .map(|info| reach(info))
        {
            Some(CastReach::LosBlocked) => unreached_reason(
                RejectionReason::LosBlocked,
                ability,
                def,
                combatant,
                my_pos,
                auras,
                ctx,
                PreCastOpts::default(),
            ),
            Some(CastReach::OutOfRange { distance }) => unreached_reason(
                RejectionReason::OutOfRange {
                    distance,
                    max: def.range,
                },
                ability,
                def,
                combatant,
                my_pos,
                auras,
                ctx,
                PreCastOpts::default(),
            ),
            _ if all.iter().any(|c| {
                spirit_endangered(c.health_pct, c.attackers) && (c.immune || c.stealthed)
            }) =>
            {
                RejectionReason::PreconditionUnmet {
                    note: "the endangered teammate is immune or hidden by stealth".into(),
                }
            }
            _ if all.iter().any(|c| c.spirited) => RejectionReason::AlreadyApplied,
            _ => RejectionReason::PreconditionUnmet {
                note: "no teammate below 40% under two attackers, or below 25%".into(),
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

/// Holy Nova: the heal a walk allows, on every teammate inside it, with its
/// damage on every enemy inside it, seen or not.
pub fn try_holy_nova(
    combat_log: &mut CombatLog,
    game_rng: &mut GameRng,
    abilities: &AbilityBook,
    entity: Entity,
    combatant: &mut Combatant,
    my_pos: Vec3,
    auras: Option<&ActiveAuras>,
    ctx: &CombatContext,
    escape_defer: Option<f32>,
    heal_hp: f32,
    heal_range: f32,
    area: &mut AreaQueues<'_>,
    builder: &mut DecisionEventBuilder<'_>,
) -> bool {
    let ability = AbilityType::HolyNova;
    let Some(def) = abilities.get(&ability) else {
        return false;
    };
    let my_team = combatant.team;
    let radius = def.range;
    let inside = |pos: Vec3| my_pos.distance(pos) <= radius;
    let healed: Vec<&super::CombatantInfo> = ctx
        .combatants
        .values()
        .filter(|c| c.team == my_team && c.is_alive && !c.is_pet && inside(c.position))
        .collect();
    // The teammate the Flash Heal would heal: its own pick (the most injured
    // in reach, below the Flash Heal line).
    let heal_target = ctx
        .lowest_health_ally_in_reach(heal_hp, heal_range, my_pos)
        .ok();
    let target_inside = heal_target.is_some_and(|t| inside(t.position));
    let deferred = match (escape_defer, heal_target) {
        (Some(threshold), Some(t)) => t.health_pct() > threshold,
        _ => false,
    };
    let threatened = deferred || interrupt_ready_on(ctx, my_team, my_pos);
    // Decided on what the Priest sees: an unseen enemy holds no crowd control
    // of its team's.
    let breaks_team_cc = ctx.combatants.values().any(|c| {
        c.team != my_team
            && c.is_alive
            && inside(c.position)
            && ctx.has_friendly_breakable_cc(c.entity)
    });
    if !holy_nova_due(target_inside, threatened, breaks_team_cc) {
        let note = if !target_inside {
            "the teammate a Flash Heal would heal is not inside the nova"
        } else if !threatened {
            "no interrupt ready and no walk: the Flash Heal goes out"
        } else {
            "an enemy inside the nova is in the team's breakable crowd control"
        };
        builder.reject(
            ability,
            RejectionReason::PreconditionUnmet { note: note.into() },
        );
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

    builder.choose(cleared, None, true);
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

    let self_auras = ctx
        .active_auras
        .get(&entity)
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    let ap_bonus = get_attack_power_bonus_from_slice(self_auras);
    let sp_bonus = get_spell_power_bonus_from_slice(self_auras);
    let crit_chance =
        combatant.crit_chance + get_crit_chance_bonus_from_slice(self_auras) + def.crit_chance;

    // The damage: every enemy inside, seen or not (`area_victims` — an area
    // does not aim, so a stealthed Rogue in it is struck and revealed).
    let victims: Vec<(Entity, Vec3)> = ctx
        .area_victims(my_team, my_pos, radius)
        .into_iter()
        .map(|info| (info.entity, info.position))
        .collect();
    for (target, target_pos) in victims {
        let mut damage =
            combatant.calculate_ability_damage_config(def, game_rng, ap_bonus, sp_bonus);
        let is_crit = roll_crit(crit_chance, game_rng);
        if is_crit {
            damage *= CRIT_DAMAGE_MULTIPLIER;
        }
        area.damage.push(QueuedAoeDamage {
            caster: entity,
            target,
            damage,
            caster_team: combatant.team,
            caster_slot: combatant.slot,
            caster_class: combatant.class,
            target_pos,
            is_crit,
            ability,
            school: def.spell_school,
        });
    }

    // The healing: every living teammate inside, the Priest included, pets
    // aside (no healer heals a pet).
    let healed: Vec<Entity> = healed.iter().map(|c| c.entity).collect();
    for target in healed {
        let mut heal = combatant.calculate_ability_healing_config(def, game_rng, sp_bonus);
        let is_crit = roll_crit(crit_chance, game_rng);
        if is_crit {
            heal *= CRIT_HEALING_MULTIPLIER;
        }
        area.heals.push(QueuedAoeHeal {
            caster: entity,
            target,
            heal,
            is_crit,
            caster_team: combatant.team,
            caster_slot: combatant.slot,
            caster_class: combatant.class,
            ability,
        });
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(raw: u32, health_pct: f32, attackers: usize) -> SpiritCandidate {
        SpiritCandidate {
            entity: Entity::from_raw(raw),
            health_pct,
            attackers,
            immune: false,
            stealthed: false,
            spirited: false,
        }
    }

    #[test]
    fn the_spirit_answers_a_focused_teammate_below_forty_or_anyone_below_twenty_five() {
        assert!(spirit_endangered(0.39, 2));
        assert!(!spirit_endangered(0.39, 1));
        assert!(!spirit_endangered(0.40, 2));
        assert!(spirit_endangered(0.24, 0));
        assert!(!spirit_endangered(0.25, 1));
    }

    #[test]
    fn the_spirit_goes_on_the_most_endangered_teammate_it_may_take() {
        let a = candidate(1, 0.35, 2);
        let b = candidate(2, 0.20, 0);
        assert_eq!(guardian_spirit_target(&[a, b]), Some(b.entity));
        assert_eq!(guardian_spirit_target(&[a]), Some(a.entity));
        assert_eq!(guardian_spirit_target(&[candidate(3, 0.35, 1)]), None);
        for held in [
            SpiritCandidate { immune: true, ..b },
            SpiritCandidate {
                stealthed: true,
                ..b
            },
            SpiritCandidate {
                spirited: true,
                ..b
            },
        ] {
            assert_eq!(guardian_spirit_target(&[a, held]), Some(a.entity));
            assert_eq!(guardian_spirit_target(&[held]), None);
        }
    }

    #[test]
    fn the_nova_replaces_a_threatened_heal_and_never_breaks_the_teams_crowd_control() {
        assert!(holy_nova_due(true, true, false));
        assert!(!holy_nova_due(false, true, false));
        assert!(!holy_nova_due(true, false, false));
        assert!(!holy_nova_due(true, true, true));
    }
}
