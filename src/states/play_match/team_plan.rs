//! Team-level strategy layer — the `TeamPlan` from
//! `design-docs/team-level-positioning-ai.md`.
//!
//! **Step 2 of the migration: this is deliberately inert.** The types, the
//! resource, and the recompute cadence all exist and run, but every plan carries
//! `anchor: None` and nothing consumes a plan yet. That is the point — landing the
//! scaffolding as a *provable* no-op means the next step changes behaviour on
//! purpose rather than by accident, and any drift it causes is attributable to it
//! alone.
//!
//! Verify the no-op with the recorded baseline, not the test suite:
//!
//! ```bash
//! scripts/behaviour_baseline.sh | diff tests/baselines/legacy_behaviour_2026-07-31.txt -
//! ```
//!
//! An empty diff means byte-identical simulation. The movement probes assert
//! *bounded* properties, so they would pass through a real behaviour change.
//!
//! ## Why three layers
//!
//! Strategy (this file) answers "what is our team trying to do", on a cadence of
//! seconds. Below it sits an obligation layer (duties owed to *another* unit —
//! peeling, screening) and then execution (the existing posture machinery). See
//! the design doc; the ordering matters because obligations are instrumental to
//! strategy, not parallel with it.
//!
//! ## Determinism
//!
//! The planner must never draw from `GameRng`. It runs inside the same schedule as
//! the AI, so consuming a random number would shift the draw order for everything
//! downstream and silently break seeded replay — the failure would look like an
//! unrelated behaviour change several systems away. Plan inputs are positions,
//! health and roster only.

use bevy::prelude::*;
use std::collections::BTreeMap;

use super::components::{Combatant, MatchCountdown};
use super::ai_profile::AiProfile;
use super::map_config::ActiveMapGeometry;
use super::map_geometry::ObstacleVolume;
use crate::states::match_config::CharacterClass;

/// Where a team wants the fight to happen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// Index into `ActiveMapGeometry::volumes` — a pillar to fight around.
    Obstacle(usize),
    /// An arbitrary spot, for open-field plans.
    Point(Vec2),
}

/// Why a team withdrew. `Bait` and `Recover` were separate stances in an early
/// draft; they share their entire exit path (into `Press` on conversion or on
/// enemy overextension) and differ only in *why* the team retreated, so they are
/// one stance with a reason code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WithdrawReason {
    /// From strength — retreat to provoke an overcommit.
    Draw,
    /// From weakness — retreat because we must.
    Recover,
}

/// What a team is trying to do right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Stance {
    /// Hold advantageous ground and make the enemy come to us.
    Hold,
    /// Press an advantage.
    #[default]
    Press,
    /// Retreat in order to counterattack.
    Withdraw(WithdrawReason),
}

/// One unit's positional job, resolved against the team's anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleIntent {
    /// Break enemy caster sight while staying in heal range of the anchor ally.
    OccupyCover,
    /// Keep sight of the partner while denying it to the enemy kill target.
    ScreenPartner,
    /// Get in ability range of the called target.
    PressTarget,
    /// Stay outside enemy threat range while keeping sight of the kill target.
    HoldRange,
    /// Converge on the team's shared anchor — the only intent where teammates
    /// deliberately stack rather than distribute.
    StackAnchor,
}

/// One team's plan.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TeamPlan {
    /// Where this team wants the fight. `None` = open field, and is what step 2
    /// always produces.
    pub anchor: Option<Anchor>,
    pub stance: Stance,
    /// The team's called kill target. The plan owns this rather than each unit
    /// choosing independently; held constant for a whole match in v1.
    pub kill_target: Option<Entity>,
    /// Per-unit positional intent. `BTreeMap` (not `HashMap`) because plan
    /// iteration order must be stable at a fixed seed — the same determinism rule
    /// every other AI collection in this codebase follows.
    pub intents: BTreeMap<Entity, RoleIntent>,
}

/// Both teams' plans.
///
/// A fixed two-element array indexed `team - 1`, not a map: teams are always
/// exactly two, so an array is deterministic by construction and there is no
/// iteration order to get wrong.
#[derive(Resource, Clone, Debug, Default)]
pub struct TeamPlans {
    plans: [TeamPlan; 2],
    /// Roster fingerprint the current plans were computed from. Recomputing only
    /// when this changes is what makes the plan an intent that PERSISTS across
    /// seconds; a per-frame plan would just be the movement scorer with extra
    /// steps.
    roster: Vec<(u8, u8)>,
    /// Number of recomputes this match. Exposed for tests and tracing — a plan
    /// layer that silently never recomputes looks identical to a working one.
    pub revisions: u32,
}

impl TeamPlans {
    /// Plan for `team` (1 or 2). Returns team 1's plan for any out-of-range value
    /// rather than panicking — a bad team id is a caller bug, not worth aborting a
    /// match over.
    pub fn for_team(&self, team: u8) -> &TeamPlan {
        &self.plans[usize::from(team.saturating_sub(1)).min(1)]
    }

    fn for_team_mut(&mut self, team: u8) -> &mut TeamPlan {
        &mut self.plans[usize::from(team.saturating_sub(1)).min(1)]
    }
}


/// A team's composition, reduced to what plan selection needs.
///
/// Deliberately coarse. The distinction that matters for a pillar camp is whether
/// a team has something that must CLOSE to deal damage (melee) alongside a healer
/// worth protecting — such a team gains by making the enemy walk to it. A team
/// that can already deal damage at range gains nothing from camping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompProfile {
    pub has_melee: bool,
    pub has_healer: bool,
    pub has_ranged: bool,
}

impl CompProfile {
    /// A melee/healer pair — the archetype that benefits from holding cover and
    /// forcing the approach.
    pub fn wants_to_camp(&self) -> bool {
        self.has_melee && self.has_healer
    }
}

/// Classify a roster of classes. Pure, so plan selection is testable without a
/// `World`.
pub fn classify_comp(classes: &[CharacterClass]) -> CompProfile {
    CompProfile {
        has_melee: classes.iter().any(|c| c.is_melee()),
        has_healer: classes.iter().any(|c| c.is_healer()),
        // A healer that is also melee (Paladin) does not make a team "ranged".
        has_ranged: classes.iter().any(|c| !c.is_melee() && !c.is_healer()),
    }
}

/// Pick the obstacle a team should hold, given its side of the arena.
///
/// Chooses the cover nearest the team's own spawn: camping the far side would
/// mean crossing the arena first, which is the opposite of making the enemy come
/// to you. Ties break on the lowest index so the choice is deterministic — the
/// four Nagrand pillars are symmetric, so ties are the normal case, not an edge.
///
/// Returns `None` when the map has no obstacles, which is what keeps this inert
/// on BasicArena without a separate map check.
pub fn choose_anchor(volumes: &[ObstacleVolume], spawn_x: f32) -> Option<Anchor> {
    let mut best: Option<(f32, usize)> = None;
    for (i, v) in volumes.iter().enumerate() {
        let (center, _) = v.footprint_disc();
        // Same side of the arena as our gate, and nearest to it.
        if center.x.signum() != spawn_x.signum() {
            continue;
        }
        let d = (center.x - spawn_x).abs();
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
        }
    }
    best.map(|(_, i)| Anchor::Obstacle(i))
}

/// Recompute both teams' plans when the roster changes.
///
/// **Step 2: produces `anchor: None` for every comp, and nothing reads the
/// result.** The cadence and the wiring are real so that step 3 only has to add
/// the decision, but the output is inert by construction.
///
/// Cadence is roster-driven rather than per-frame: gates opening and any
/// combatant dying are the events that invalidate a plan. Pets are excluded — a
/// pet dying does not change what a team is trying to do.
pub fn update_team_plans(
    countdown: Res<MatchCountdown>,
    combatants: Query<&Combatant>,
    geometry: Option<Res<ActiveMapGeometry>>,
    profile: Option<Res<AiProfile>>,
    mut plans: ResMut<TeamPlans>,
) {
    if !countdown.gates_opened {
        return;
    }

    // (team, slot) rather than Entity: stable across the match, and meaningful in
    // a debugger. Sorted so the fingerprint does not depend on query order.
    let mut roster: Vec<(u8, u8)> = combatants
        .iter()
        .filter(|c| c.is_alive() && c.slot < PET_SLOT_BASE)
        .map(|c| (c.team, c.slot))
        .collect();
    roster.sort_unstable();

    // Read through Deref — taking ResMut mutably here would mark the resource
    // changed every frame and defeat Bevy's change detection for consumers.
    if roster == plans.roster {
        return;
    }

    plans.roster = roster;
    plans.revisions += 1;

    // Legacy keeps the inert plan, so every recorded baseline stays reproducible.
    // Only the TeamPlan profile selects a real plan.
    let team_plan_active = profile.map(|p| p.is_team_plan()).unwrap_or(false);
    let volumes: &[ObstacleVolume] = geometry
        .as_ref()
        .map(|g| g.volumes.as_slice())
        .unwrap_or(&[]);

    for team in [1u8, 2u8] {
        if !team_plan_active {
            *plans.for_team_mut(team) = TeamPlan::default();
            continue;
        }

        let classes: Vec<CharacterClass> = combatants
            .iter()
            .filter(|c| c.team == team && c.is_alive() && c.slot < PET_SLOT_BASE)
            .map(|c| c.class)
            .collect();
        let comp = classify_comp(&classes);

        // Team 1 spawns at -x, team 2 at +x. Camping the cover nearest our own
        // gate is what makes the enemy walk to us; camping theirs would mean
        // crossing the arena first.
        let spawn_side = if team == 1 { -1.0 } else { 1.0 };

        // A camp is only meaningful for a comp that must close to deal damage,
        // and only where there is cover to hold. `choose_anchor` returns None on
        // an obstacle-free map, so BasicArena needs no separate guard.
        let anchor = comp
            .wants_to_camp()
            .then(|| choose_anchor(volumes, spawn_side))
            .flatten();

        let plan = plans.for_team_mut(team);
        plan.anchor = anchor;
        // Hold ground and make them come; without an anchor there is nothing to
        // hold, so fall back to today's behaviour.
        plan.stance = if anchor.is_some() { Stance::Hold } else { Stance::Press };
        plan.intents.clear();
    }
}

/// Slots at or above this are pets. Mirrors the convention in `acquire_targets`.
const PET_SLOT_BASE: u8 = 100;

#[cfg(test)]
mod tests {
    use super::*;


    use crate::states::match_config::CharacterClass;

    /// Drive `update_team_plans` over a real `World`, so the cadence is tested as
    /// wired rather than as intended.
    fn run_planner(gates_open: bool, roster: &[(u8, u8, bool)]) -> TeamPlans {
        let mut app = App::new();
        app.insert_resource(MatchCountdown {
            time_remaining: 0.0,
            gates_opened: gates_open,
        });
        app.insert_resource(TeamPlans::default());
        for &(team, slot, alive) in roster {
            let mut c = Combatant::new(team, slot, CharacterClass::Warrior);
            if !alive {
                c.current_health = 0.0;
            }
            app.world_mut().spawn(c);
        }
        app.add_systems(Update, update_team_plans);
        app.update();
        app.world().resource::<TeamPlans>().clone()
    }

    /// The planner must NOT run before gates open — a plan formed during the
    /// countdown would be based on starting positions nobody fights from.
    #[test]
    fn does_not_plan_before_gates_open() {
        let plans = run_planner(false, &[(1, 0, true), (2, 0, true)]);
        assert_eq!(plans.revisions, 0);
        assert!(plans.roster.is_empty());
    }

    /// The load-bearing check behind the step-2 no-op claim: the planner really
    /// DOES run. A system that silently never executes would also produce a
    /// byte-identical baseline, so "nothing changed" only means something once
    /// this passes.
    #[test]
    fn plans_once_gates_are_open() {
        let plans = run_planner(true, &[(1, 0, true), (1, 1, true), (2, 0, true)]);
        assert_eq!(plans.revisions, 1, "planner should have recomputed exactly once");
        assert_eq!(plans.roster, vec![(1, 0), (1, 1), (2, 0)]);
        // ...and step 2's output is still inert.
        for team in [1u8, 2u8] {
            assert_eq!(plans.for_team(team).anchor, None);
        }
    }

    /// Dead combatants leave the roster, so a death triggers a replan — that is
    /// the cadence, not a per-frame recompute.
    #[test]
    fn dead_combatants_are_excluded_from_the_roster() {
        let plans = run_planner(true, &[(1, 0, true), (1, 1, false), (2, 0, true)]);
        assert_eq!(plans.roster, vec![(1, 0), (2, 0)], "dead slot 1 should be gone");
    }

    /// Pets must not drive replanning — a pet dying does not change what a team is
    /// trying to do, and letting it churn the plan would make the cadence noise.
    #[test]
    fn pets_do_not_enter_the_roster() {
        let plans = run_planner(true, &[(1, 0, true), (1, PET_SLOT_BASE, true), (2, 0, true)]);
        assert_eq!(plans.roster, vec![(1, 0), (2, 0)], "pet slot should be excluded");
    }

    /// A stable roster must not recompute — the plan is an intent that persists,
    /// not a per-frame derivation.
    #[test]
    fn stable_roster_does_not_replan() {
        let mut app = App::new();
        app.insert_resource(MatchCountdown { time_remaining: 0.0, gates_opened: true });
        app.insert_resource(TeamPlans::default());
        app.world_mut().spawn(Combatant::new(1, 0, CharacterClass::Warrior));
        app.world_mut().spawn(Combatant::new(2, 0, CharacterClass::Warrior));
        app.add_systems(Update, update_team_plans);
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<TeamPlans>().revisions,
            1,
            "five frames with an unchanged roster should yield ONE recompute"
        );
    }


    use crate::states::play_match::map_geometry::ObstacleVolume;

    fn pillar(x: f32, z: f32) -> ObstacleVolume {
        ObstacleVolume::Prism {
            center_xz: Vec2::new(x, z),
            circumradius: 6.0,
            sides: 8,
            rotation: 0.0,
            base_y: 0.0,
            height: 5.0,
        }
    }

    /// The four Nagrand pillars, in maps.ron declaration order.
    fn nagrand() -> Vec<ObstacleVolume> {
        vec![
            pillar(-40.0, -20.0),
            pillar(-40.0, 20.0),
            pillar(40.0, -20.0),
            pillar(40.0, 20.0),
        ]
    }

    #[test]
    fn melee_plus_healer_wants_to_camp() {
        let comp = classify_comp(&[CharacterClass::Warrior, CharacterClass::Priest]);
        assert!(comp.wants_to_camp(), "melee + healer is the camping archetype");
    }

    /// A team that already deals damage at range gains nothing from making the
    /// enemy walk to it.
    #[test]
    fn double_ranged_does_not_camp() {
        let comp = classify_comp(&[CharacterClass::Mage, CharacterClass::Warlock]);
        assert!(!comp.wants_to_camp());
        assert!(comp.has_ranged);
    }

    /// A Paladin is both melee and healer, so it satisfies both halves alone —
    /// and must NOT be counted as ranged.
    #[test]
    fn paladin_counts_as_melee_and_healer_but_not_ranged() {
        let comp = classify_comp(&[CharacterClass::Paladin]);
        assert!(comp.has_melee && comp.has_healer);
        assert!(!comp.has_ranged, "a melee healer must not make the team ranged");
        assert!(comp.wants_to_camp());
    }

    /// Each team camps cover on ITS OWN side. Camping the far pillars would mean
    /// crossing the arena first, which defeats the purpose.
    #[test]
    fn anchor_is_on_the_teams_own_side() {
        let v = nagrand();
        // Team 1 spawns at -x.
        let a1 = choose_anchor(&v, -1.0).expect("a -x pillar exists");
        let Anchor::Obstacle(i1) = a1 else { panic!("expected an obstacle anchor") };
        assert!(matches!(i1, 0 | 1), "team 1 must anchor on a -x pillar, got {i1}");

        let a2 = choose_anchor(&v, 1.0).expect("a +x pillar exists");
        let Anchor::Obstacle(i2) = a2 else { panic!("expected an obstacle anchor") };
        assert!(matches!(i2, 2 | 3), "team 2 must anchor on a +x pillar, got {i2}");
    }

    /// Symmetric pillars make ties the NORMAL case here, not an edge case, so the
    /// tie-break has to be deterministic or plans differ run to run.
    #[test]
    fn anchor_choice_is_deterministic_under_ties() {
        let v = nagrand();
        let first = choose_anchor(&v, -1.0);
        for _ in 0..16 {
            assert_eq!(choose_anchor(&v, -1.0), first);
        }
    }

    /// No obstacles means no anchor — which is what keeps this inert on
    /// BasicArena without a separate map check.
    #[test]
    fn no_obstacles_yields_no_anchor() {
        assert_eq!(choose_anchor(&[], -1.0), None);
        assert_eq!(choose_anchor(&[], 1.0), None);
    }

    #[test]
    fn defaults_are_an_inert_open_field_plan() {
        let plans = TeamPlans::default();
        for team in [1u8, 2u8] {
            let p = plans.for_team(team);
            assert_eq!(p.anchor, None, "step 2 must produce no anchor");
            assert_eq!(p.stance, Stance::Press);
            assert_eq!(p.kill_target, None);
            assert!(p.intents.is_empty());
        }
        assert_eq!(plans.revisions, 0);
    }

    /// Teams index independently — writing one must not alias the other.
    #[test]
    fn teams_index_independently() {
        let mut plans = TeamPlans::default();
        plans.for_team_mut(1).stance = Stance::Hold;
        assert_eq!(plans.for_team(1).stance, Stance::Hold);
        assert_eq!(plans.for_team(2).stance, Stance::Press);
    }

    /// A bad team id must not panic mid-match. 0 and 3+ clamp into range.
    #[test]
    fn out_of_range_team_ids_clamp_instead_of_panicking() {
        let plans = TeamPlans::default();
        for team in [0u8, 1, 2, 3, 255] {
            let _ = plans.for_team(team);
        }
    }

    /// `Withdraw` carries its reason, so `Draw` and `Recover` stay distinguishable
    /// despite sharing a stance and an exit path.
    #[test]
    fn withdraw_reasons_are_distinct() {
        assert_ne!(
            Stance::Withdraw(WithdrawReason::Draw),
            Stance::Withdraw(WithdrawReason::Recover)
        );
        assert_ne!(Stance::Withdraw(WithdrawReason::Recover), Stance::Press);
    }
}
