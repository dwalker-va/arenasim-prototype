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
    for team in [1u8, 2u8] {
        // Step 2 is a no-op: an open-field plan with no anchor and no intents,
        // which is exactly today's behaviour. Step 3 replaces this with
        // comp-matchup selection.
        *plans.for_team_mut(team) = TeamPlan::default();
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
