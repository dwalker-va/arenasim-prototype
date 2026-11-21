//! The team's target focus: leave a target nobody can see (AS-389).
//!
//! A ranged DPS used to keep its target for as long as that target was valid,
//! and sight was not part of valid. Against an enemy healer hiding behind a
//! pillar it tunnelled a target it could not shoot while the enemy DPS that
//! healer had left exposed stood in reach and in sight. The design and its
//! measurements are `docs/design/2026-10-25-ranged-dps-vs-hidden-targets.md`.
//!
//! **The situation** ([`classify`], `TeamSituation::target_focus`). `TeamPlan`
//! only, on a map with obstacles. Each frame the team keeps, per ranged DPS
//! (Mage, Hunter, Warlock), how long its target has been out of its sight
//! ([`TargetSight`]). When one has been blind to its target for
//! [`FOCUS_HIDDEN_SECS`] and another enemy stands within [`FOCUS_REACH`] of it
//! and in its sight, the team's focus moves to the nearest such enemy
//! ([`Focus`]). The team decides; its ranged DPS take the focus in target
//! acquisition ([`retarget`], `combat_ai::acquire_targets`).
//!
//! **Anti-strobe.** A focus holds [`FOCUS_DWELL`] before it can move again.
//! There is no separate return rule: the team goes back to its old target by
//! the same rule that left it — once the focus has itself been out of a ranged
//! DPS's sight for [`FOCUS_HIDDEN_SECS`] and the old target stands in sight.
//! So a target that juked out of sight once is not returned to the moment it
//! steps back, and a pair of enemies trading cover cannot flip the focus
//! faster than the hidden clock and the dwell allow.
//!
//! **What it does not move.** A DPS on the team's kill window subject (the
//! Burst play's) does not leave it — neither to move the focus ([`classify`])
//! nor to take one ([`retarget`]); a configured kill target (the player's
//! kill call) wins over the focus; the Hunter's Freezing Trap retarget still
//! runs after it. Melee DPS do not take the focus: their target-blind time is
//! under 1% of their life (AS-385 §2.2), they have their own swap (the melee
//! bucket A), and a team focus across the arena would cost them their swing
//! time.

use std::collections::BTreeMap;

use bevy::prelude::*;
use smallvec::SmallVec;

use super::{grants_damage_immunity, stealth_visible, CombatantInfo};
use crate::states::play_match::components::{Aura, AuraType};
use crate::states::play_match::decision_trace::SituationChangeKind;
use crate::states::play_match::map_geometry::{has_line_of_sight, ObstacleVolume};
use crate::states::play_match::situation::Role;

/// Seconds a ranged DPS's target must have been out of its sight before the
/// focus may move off it. The prototype measured a 0.5s and a 2.0s delay
/// within half a point of each other (design §5); 1.5s, one global cooldown,
/// lets a target that steps out between two casts come back without a
/// switch: 24% of the blind time is in episodes shorter than 1.5s (§3.1).
pub const FOCUS_HIDDEN_SECS: f32 = 1.5;

/// Seconds a focus holds before it can move again.
pub const FOCUS_DWELL: f32 = 2.0;

/// Yards within which a ranged DPS can take an enemy as the focus: Auto
/// Shot's 35yd, the shortest of the three ranged classes' main attacks
/// (Frostbolt and Shadow Bolt reach 40).
pub const FOCUS_REACH: f32 = 35.0;

/// Whether a unit playing `role` follows the team's focus: the ranged DPS
/// (a Shadowform Priest among them).
pub fn follows_focus(role: Role) -> bool {
    role == Role::RangedDps
}

/// One ranged DPS's sight of its own target, carried frame to frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetSight {
    pub dps: Entity,
    pub target: Entity,
    /// When the DPS last lost sight of this target; `None` while it sees it.
    pub blind_since: Option<f32>,
}

/// The team's focus: the enemy its ranged DPS shoot, the target it moved off,
/// and when.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Focus {
    pub subject: Entity,
    /// The target the focus moved off: out of the deciding DPS's sight for
    /// [`FOCUS_HIDDEN_SECS`] when it moved.
    pub from: Entity,
    pub since: f32,
}

fn has_shadow_sight(auras: &BTreeMap<Entity, Vec<Aura>>, entity: Entity) -> bool {
    auras
        .get(&entity)
        .is_some_and(|list| list.iter().any(|a| a.effect_type == AuraType::ShadowSight))
}

fn immune(auras: &BTreeMap<Entity, Vec<Aura>>, entity: Entity) -> bool {
    auras
        .get(&entity)
        .is_some_and(|list| list.iter().any(|a| grants_damage_immunity(a.effect_type)))
}

/// Whether `viewer` can take `enemy` as a target: a living non-pet enemy it
/// can see through stealth, not immune to damage — the validity
/// `acquire_targets` keeps a target by.
fn targetable(
    auras: &BTreeMap<Entity, Vec<Aura>>,
    viewer: &CombatantInfo,
    enemy: &CombatantInfo,
) -> bool {
    enemy.is_alive
        && !enemy.is_pet
        && enemy.team != viewer.team
        && stealth_visible(
            enemy.stealthed,
            has_shadow_sight(auras, viewer.entity),
            has_shadow_sight(auras, enemy.entity),
        )
        && !immune(auras, enemy.entity)
}

/// The cast's own sight test, eye to eye on the units' positions.
fn sees(obstacles: &[ObstacleVolume], from: Vec3, to: Vec3) -> bool {
    has_line_of_sight(obstacles, from, to)
}

/// Whether `dps` could take `enemy` as its focus: a target it can take
/// ([`targetable`]), within [`FOCUS_REACH`] and in its sight — the test a
/// focus moves onto an enemy by ([`classify`]).
pub fn could_take(
    auras: &BTreeMap<Entity, Vec<Aura>>,
    obstacles: &[ObstacleVolume],
    dps: &CombatantInfo,
    enemy: &CombatantInfo,
) -> bool {
    targetable(auras, dps, enemy)
        && enemy.position.distance(dps.position) <= FOCUS_REACH
        && sees(obstacles, dps.position, enemy.position)
}

/// Whether an enemy of `healer` that follows a focus ([`follows_focus`])
/// could turn on one of the healer's living teammates ([`could_take`]): the
/// case in which the healer's cover from it only hands its fire to that
/// teammate (AS-440, `team_solve::SolveWorld::cover_waived`). `false` on a
/// map with no obstacles, where no focus ever moves. `roster` is the view
/// the healer decides from. Whether the enemy team runs a focus at all is
/// asked once per team in `situation::update_situations`
/// (`TeamSituation::handoff_rule`), not here.
pub fn could_turn_on_teammate(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    obstacles: &[ObstacleVolume],
    healer: &CombatantInfo,
) -> bool {
    !obstacles.is_empty()
        && roster
            .values()
            .filter(|d| d.team != healer.team && d.is_alive && !d.is_pet && follows_focus(d.role))
            .any(|d| {
                roster.values().any(|mate| {
                    mate.team == healer.team
                        && mate.entity != healer.entity
                        && could_take(auras, obstacles, d, mate)
                })
            })
}

/// The nearest enemy other than `besides` that `dps` can take: in
/// [`FOCUS_REACH`] and in its sight. Entity order on a tie.
fn nearest_in_sight(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    obstacles: &[ObstacleVolume],
    dps: &CombatantInfo,
    besides: Entity,
) -> Option<Entity> {
    roster
        .values()
        .filter(|e| e.entity != besides && could_take(auras, obstacles, dps, e))
        .min_by(|a, b| {
            a.position
                .distance(dps.position)
                .total_cmp(&b.position.distance(dps.position))
        })
        .map(|e| e.entity)
}

/// The team's focus and its ranged DPS's sight clocks this frame. `enabled`
/// is `SituationConfig::target_focus` (`TeamPlan`, not withheld);
/// `kill_subject` the team's kill window subject, which no DPS leaves.
#[allow(clippy::too_many_arguments)]
pub fn classify(
    roster: &BTreeMap<Entity, CombatantInfo>,
    auras: &BTreeMap<Entity, Vec<Aura>>,
    obstacles: &[ObstacleVolume],
    team: u8,
    prev_focus: Option<Focus>,
    prev_sight: &[TargetSight],
    kill_subject: Option<Entity>,
    now: f32,
    enabled: bool,
) -> (Option<Focus>, SmallVec<[TargetSight; 3]>) {
    // Off, or a map with nothing to hide behind (nobody is ever blind there).
    if !enabled || obstacles.is_empty() {
        return (None, SmallVec::new());
    }
    let dps: SmallVec<[&CombatantInfo; 3]> = roster
        .values()
        .filter(|i| i.team == team && i.is_alive && !i.is_pet && follows_focus(i.role))
        .collect();
    let sight: SmallVec<[TargetSight; 3]> = dps
        .iter()
        .filter_map(|d| {
            let target = roster.get(&d.target?)?;
            if !target.is_alive || target.team == team {
                return None;
            }
            let blind = !sees(obstacles, d.position, target.position);
            let carried = prev_sight
                .iter()
                .find(|s| s.dps == d.entity && s.target == target.entity)
                .and_then(|s| s.blind_since);
            Some(TargetSight {
                dps: d.entity,
                target: target.entity,
                blind_since: blind.then(|| carried.unwrap_or(now)),
            })
        })
        .collect();
    if dps.is_empty() {
        return (None, sight);
    }
    // The held focus, while its subject is still a target the team can take.
    let held = prev_focus.filter(|f| {
        roster
            .get(&f.subject)
            .is_some_and(|s| dps.iter().any(|d| targetable(auras, d, s)))
    });
    if held.is_some_and(|f| now - f.since < FOCUS_DWELL) {
        return (held, sight);
    }
    for s in &sight {
        let Some(since) = s.blind_since else {
            continue;
        };
        if now - since < FOCUS_HIDDEN_SECS || kill_subject == Some(s.target) {
            continue;
        }
        let Some(d) = roster.get(&s.dps) else {
            continue;
        };
        if let Some(subject) = nearest_in_sight(roster, auras, obstacles, d, s.target) {
            if held.is_some_and(|f| f.subject == subject) {
                return (held, sight);
            }
            return (
                Some(Focus {
                    subject,
                    from: s.target,
                    since: now,
                }),
                sight,
            );
        }
    }
    (held, sight)
}

/// How the focus moved between two classifications, if it did.
pub fn change(prev: Option<Focus>, next: Option<Focus>) -> Option<(SituationChangeKind, Focus)> {
    match (prev, next) {
        (None, None) => None,
        (None, Some(f)) => Some((SituationChangeKind::Onset, f)),
        (Some(f), None) => Some((SituationChangeKind::End, f)),
        (Some(a), Some(b)) if a.subject != b.subject => Some((SituationChangeKind::Onset, b)),
        _ => None,
    }
}

/// The target a ranged DPS takes from its team's focus, if it should switch
/// to it now: the focus names an enemy other than its target, the DPS can
/// take it, and either it stands within [`FOCUS_REACH`] and in the DPS's
/// sight or the DPS cannot see its own target either — unless its target is
/// the team's kill window subject (`kill_subject`), which no DPS leaves.
/// `None` leaves the target as acquisition left it. `targetable` is
/// acquisition's own
/// validity (alive, visible through stealth, not immune, not a pet) and
/// `position` each unit's position.
#[allow(clippy::too_many_arguments)]
pub fn retarget(
    focus: Option<Focus>,
    me: Entity,
    role: Role,
    my_pos: Vec3,
    current: Option<Entity>,
    kill_subject: Option<Entity>,
    obstacles: &[ObstacleVolume],
    targetable: impl Fn(Entity) -> bool,
    position: impl Fn(Entity) -> Option<Vec3>,
) -> Option<Entity> {
    let focus = focus?;
    if !follows_focus(role) || current == Some(focus.subject) || focus.subject == me {
        return None;
    }
    if current.is_some() && current == kill_subject {
        return None;
    }
    if !targetable(focus.subject) {
        return None;
    }
    let at = position(focus.subject)?;
    let reaches = at.distance(my_pos) <= FOCUS_REACH && sees(obstacles, my_pos, at);
    let own_hidden = current
        .and_then(&position)
        .is_some_and(|p| !sees(obstacles, my_pos, p));
    (reaches || own_hidden).then_some(focus.subject)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `could_turn_on_teammate` (AS-440): an enemy ranged DPS at the origin,
    /// our healer and its Warrior somewhere around it, the reach fixture's
    /// pillar 8yd down +X. Each case is the Warrior in the Mage's sight and
    /// focus reach, against one condition removed: the pillar between them,
    /// beyond `FOCUS_REACH`, an enemy that follows no focus, a map with no
    /// obstacles (where no focus moves), and a teammate the Mage could not
    /// take (`targetable`): dead, a pet, stealthed, immune to damage.
    #[test]
    fn a_teammate_in_a_ranged_dps_reach_and_sight_can_be_turned_on() {
        use crate::states::match_config::CharacterClass::{Druid, Mage, Priest, Warrior};
        use crate::states::play_match::class_ai::reach_fixture::{
            pillar, roster, unit, BEHIND, IN_SIGHT,
        };
        let mut world = World::new();
        let (enemy, healer, mate) = (
            world.spawn_empty().id(),
            world.spawn_empty().id(),
            world.spawn_empty().id(),
        );
        let none = BTreeMap::new();
        let with = |mate_info: &dyn Fn(&mut CombatantInfo),
                    enemy_class,
                    mate_at: Vec3,
                    obstacles: &[ObstacleVolume],
                    auras: &BTreeMap<Entity, Vec<Aura>>| {
            let mut m = unit(mate, 1, Warrior, mate_at, 1.0);
            mate_info(&mut m);
            let r = roster(vec![
                unit(enemy, 2, enemy_class, Vec3::new(0.0, 1.0, 0.0), 1.0),
                unit(healer, 1, Priest, Vec3::new(-6.0, 1.0, 6.0), 1.0),
                m,
            ]);
            could_turn_on_teammate(&r, auras, obstacles, &r[&healer])
        };
        let case = |enemy_class, mate_at: Vec3, obstacles: &[ObstacleVolume]| {
            with(&|_| {}, enemy_class, mate_at, obstacles, &none)
        };
        let untakeable = |f: &dyn Fn(&mut CombatantInfo)| with(f, Mage, IN_SIGHT, &pillar(), &none);
        assert!(!untakeable(&|m| m.is_alive = false), "a dead teammate");
        assert!(!untakeable(&|m| m.is_pet = true), "a pet");
        assert!(!untakeable(&|m| m.stealthed = true), "a stealthed teammate");
        let immune = BTreeMap::from([(
            mate,
            vec![Aura {
                effect_type: AuraType::DamageImmunity,
                ..Default::default()
            }],
        )]);
        assert!(
            !with(&|_| {}, Mage, IN_SIGHT, &pillar(), &immune),
            "a teammate immune to damage"
        );
        assert!(case(Mage, IN_SIGHT, &pillar()));
        assert!(
            !case(Mage, BEHIND, &pillar()),
            "the pillar hides the Warrior"
        );
        assert!(
            !case(Mage, Vec3::new(0.0, 1.0, FOCUS_REACH + 1.0), &pillar()),
            "beyond the focus's reach"
        );
        assert!(
            !case(Druid, IN_SIGHT, &pillar()),
            "a Druid follows no focus"
        );
        assert!(!case(Mage, IN_SIGHT, &[]), "no obstacles, no focus");
    }

    /// A focus's onset, a move to a new subject (a new onset), its end, and
    /// nothing while it holds.
    #[test]
    fn change_records_transitions_only() {
        let mut world = World::new();
        let (a, b, c) = (
            world.spawn_empty().id(),
            world.spawn_empty().id(),
            world.spawn_empty().id(),
        );
        let on_b = Focus {
            subject: b,
            from: a,
            since: 1.0,
        };
        let on_c = Focus {
            subject: c,
            from: b,
            since: 4.0,
        };
        assert_eq!(change(None, None), None);
        assert_eq!(
            change(None, Some(on_b)),
            Some((SituationChangeKind::Onset, on_b))
        );
        assert_eq!(change(Some(on_b), Some(on_b)), None);
        assert_eq!(
            change(Some(on_b), Some(on_c)),
            Some((SituationChangeKind::Onset, on_c))
        );
        assert_eq!(
            change(Some(on_c), None),
            Some((SituationChangeKind::End, on_c))
        );
    }
}
