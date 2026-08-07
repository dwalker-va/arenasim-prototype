//! Call-change detection: the front of the banter pipeline.
//!
//! Diffs both teams' live kill-target calls against what was last seen and
//! queues one [`CallChange`] per team that moved, tagged with the pool the
//! change should draw from. `resolver` turns a change's context into an
//! exchange; `scheduler` drains the queue and paces the beats. Nothing here
//! knows about exchanges or bubbles — it only reports that a call moved.

use bevy::prelude::*;

use super::super::banter_config::BanterContext;
use super::super::components::MatchCountdown;
use super::super::match_config::MatchConfig;

// =============================================================================
// Change detection (KTD4)
// =============================================================================

/// What the watcher last saw for one team's call.
///
/// The `NeverObserved` sentinel is the whole reason this is an enum rather than
/// a bare `Option<usize>`: it distinguishes "this team has no call" (`Seen(None)`)
/// from "we have not looked yet" (`NeverObserved`). Match start is then just a
/// change from the sentinel, so the opening exchange needs no separate
/// match-start code path — the first frame of a match reports a change for both
/// teams and the context derivation turns it into `Opening`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum LastSeenCall {
    /// No observation has been made for this team yet this match.
    #[default]
    NeverObserved,
    /// The call as of the last observation (`None` = the call was cleared).
    Seen(Option<usize>),
}

impl LastSeenCall {
    /// Whether the live `current` call differs from what was last seen.
    ///
    /// `NeverObserved` differs from everything, including `None` — that is the
    /// sentinel doing its job, not an accident.
    fn differs_from(&self, current: Option<usize>) -> bool {
        match self {
            LastSeenCall::NeverObserved => true,
            LastSeenCall::Seen(previous) => *previous != current,
        }
    }

    /// The previously-called slot, if there was an observation carrying one.
    ///
    /// Collapses both "never looked" and "explicitly cleared" to `None`, which
    /// is what the `{prev_target}` substitution wants: in either case there is
    /// no previous target to name.
    pub(super) fn slot(&self) -> Option<usize> {
        match self {
            LastSeenCall::NeverObserved => None,
            LastSeenCall::Seen(previous) => *previous,
        }
    }
}

/// One detected call change, carrying everything the banter layer needs to
/// pick and schedule an exchange without re-reading the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CallChange {
    /// Which team's call moved — `1` or `2`, matching `Combatant::team`.
    pub team: u8,
    /// The call as it now stands (`None` = cleared).
    pub new_call: Option<usize>,
    /// What the watcher had last seen, sentinel included. Kept as the full
    /// `LastSeenCall` rather than flattened to `Option<usize>` so a consumer
    /// can still tell "first call of the match" from "call cleared to nothing"
    /// after the fact.
    pub previous: LastSeenCall,
    /// Gate state at the instant of observation. Retained alongside `context`
    /// because the scheduler's beat pacing keys off whether the countdown is
    /// still running, not only off which exchange pool was selected.
    pub gates_opened: bool,
    /// Which banter pool this change should draw from, derived here so the
    /// rule lives in one place and is unit-testable.
    pub context: BanterContext,
}

/// Maps a change to the exchange pool it should draw from.
///
/// The three contexts are one mechanism seen in three situations, so the rule
/// is deliberately tiny and total:
/// - gates open   -> `Switch`     (a single-beat shout mid-fight)
/// - gates closed, nothing seen before -> `Opening`    (the countdown exchange)
/// - gates closed, a call was replaced -> `Correction` (`{prev_target}` is live)
pub(super) fn banter_context_for(previous: LastSeenCall, gates_opened: bool) -> BanterContext {
    if gates_opened {
        BanterContext::Switch
    } else if previous == LastSeenCall::NeverObserved {
        BanterContext::Opening
    } else {
        BanterContext::Correction
    }
}

/// Diffs both teams' calls against what was last seen and returns one change
/// per team that moved.
///
/// Pure over plain values so the interesting logic is testable without a Bevy
/// `World` — [`watch_kill_target_calls`] is only the resource plumbing around
/// it. Index 0 of each array is team 1, index 1 is team 2.
///
/// The two teams are diffed independently, so both changing on the same frame
/// yields two changes in team order. An empty return is the overwhelmingly
/// common case and allocates nothing (`Vec::new` defers its allocation to the
/// first push).
fn detect_call_changes(
    last_seen: [LastSeenCall; 2],
    current: [Option<usize>; 2],
    gates_opened: bool,
) -> Vec<CallChange> {
    let mut changes = Vec::new();
    for index in 0..2 {
        let previous = last_seen[index];
        let new_call = current[index];
        if !previous.differs_from(new_call) {
            continue;
        }
        changes.push(CallChange {
            team: index as u8 + 1,
            new_call,
            previous,
            gates_opened,
            context: banter_context_for(previous, gates_opened),
        });
    }
    changes
}

// =============================================================================
// Watcher resource
// =============================================================================

/// The graphical-only watcher: the last-seen call per team plus the changes
/// waiting for the beat scheduler.
///
/// DELIVERY: a drained queue on this resource rather than a Bevy `Event`.
/// Three reasons, in order of weight:
///  1. This codebase has no custom Bevy events at all — cross-system signalling
///     is done with marker components (`CastEnding`, the swing signals) and
///     resource state. A queue matches that idiom; an `add_event` would be the
///     first of its kind for a single producer/single consumer pair.
///  2. `Events<T>` double-buffers with a two-frame lifetime and its own cleanup
///     system that is NOT state-gated, so a change emitted on the last frame of
///     a match could still be readable after the state transition. A queue that
///     `reset_call_watcher_on_exit` clears has no such tail.
///  3. The watcher already owns per-team state that must reset per match, so
///     the queue rides along on a resource that exists anyway.
///
/// `play_banter_beats` drains the queue via [`CallWatcher::take_pending`]
/// every frame. The queue is bounded in practice regardless: two entries at
/// match start plus one per operator call change, all discarded on match exit.
#[derive(Resource, Debug, Default)]
pub struct CallWatcher {
    /// Last-seen call, index 0 = team 1, index 1 = team 2.
    last_seen: [LastSeenCall; 2],
    /// Changes detected but not yet consumed, oldest first.
    ///
    /// Visible to the rest of `banter` rather than private to this file only so
    /// the scheduler's plumbing tests can queue a change by hand the way this
    /// system would. Production code outside here only ever drains it, via
    /// [`CallWatcher::take_pending`].
    pub(super) pending: Vec<CallChange>,
}

impl CallWatcher {
    /// Removes and returns every queued change, leaving the queue empty.
    pub(super) fn take_pending(&mut self) -> Vec<CallChange> {
        std::mem::take(&mut self.pending)
    }
}

// =============================================================================
// Systems (graphical only — registered in `StatesPlugin::build()`)
// =============================================================================

/// Diffs the live [`MatchConfig`] calls against the watcher each frame and
/// queues any that moved.
///
/// Deliberately NOT `Res<MatchConfig>::is_changed()` (KTD4). A `ResMut` deref
/// marks the resource changed whether or not a field actually moved — the
/// in-match call control takes a `ResMut` to write one team's call and would
/// therefore flag the other team too — and `is_changed()` is true on the first
/// run after insert, which would need its own match-start special case. The
/// explicit diff has neither problem.
///
/// `MatchCountdown` is optional so the system is inert on any frame where the
/// match resources are not yet inserted; skipping the observation entirely
/// (rather than assuming a gate state) means the change is simply reported on
/// the next frame with the real gate state attached.
pub fn watch_kill_target_calls(
    config: Res<MatchConfig>,
    countdown: Option<Res<MatchCountdown>>,
    mut watcher: ResMut<CallWatcher>,
) {
    let Some(countdown) = countdown else {
        return;
    };

    let current = [config.team1_kill_target, config.team2_kill_target];
    let changes = detect_call_changes(watcher.last_seen, current, countdown.gates_opened);

    if changes.is_empty() {
        return;
    }

    watcher.pending.extend(changes);
    watcher.last_seen = [
        LastSeenCall::Seen(current[0]),
        LastSeenCall::Seen(current[1]),
    ];
}

/// Clears the watcher on leaving a match so the next one starts from the
/// sentinel and reports its own opening change.
///
/// This is why the watcher needs no per-match setup in `play_match/mod.rs`:
/// the resource is `init_resource`d once for the app and reset at the state
/// boundary, the same lifecycle `Selection` / `reset_selection_on_exit` uses.
/// Clearing `pending` as well as `last_seen` means an unconsumed change cannot
/// leak into the next match's queue.
pub fn reset_call_watcher_on_exit(mut watcher: ResMut<CallWatcher>) {
    *watcher = CallWatcher::default();
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Both teams unobserved — the state a freshly-reset watcher is in.
    const FRESH: [LastSeenCall; 2] = [LastSeenCall::NeverObserved; 2];

    #[test]
    fn first_observation_reports_a_change_from_nothing_for_each_team() {
        // The shipped defaults: both teams call enemy slot 0.
        let changes = detect_call_changes(FRESH, [Some(0), Some(0)], false);

        assert_eq!(changes.len(), 2, "both teams should report their first call");
        assert_eq!(changes[0].team, 1);
        assert_eq!(changes[1].team, 2);
        for change in &changes {
            assert_eq!(change.previous, LastSeenCall::NeverObserved);
            assert_eq!(change.new_call, Some(0));
            assert_eq!(change.context, BanterContext::Opening);
        }
    }

    #[test]
    fn unchanged_calls_report_nothing() {
        let seen = [
            LastSeenCall::Seen(Some(0)),
            LastSeenCall::Seen(Some(1)),
        ];
        assert!(detect_call_changes(seen, [Some(0), Some(1)], false).is_empty());
        // Gate state alone must not manufacture a change — the gates opening
        // mid-match is not itself a call change.
        assert!(detect_call_changes(seen, [Some(0), Some(1)], true).is_empty());
    }

    #[test]
    fn a_changed_call_reports_both_new_and_previous() {
        let seen = [LastSeenCall::Seen(Some(0)), LastSeenCall::Seen(Some(0))];
        let changes = detect_call_changes(seen, [Some(2), Some(0)], false);

        assert_eq!(changes.len(), 1, "only team 1 moved");
        let change = changes[0];
        assert_eq!(change.team, 1);
        assert_eq!(change.new_call, Some(2));
        assert_eq!(change.previous, LastSeenCall::Seen(Some(0)));
        assert_eq!(change.previous.slot(), Some(0));
    }

    #[test]
    fn both_teams_changing_on_one_frame_report_two_independent_changes() {
        let seen = [LastSeenCall::Seen(Some(0)), LastSeenCall::Seen(Some(0))];
        let changes = detect_call_changes(seen, [Some(1), Some(2)], true);

        assert_eq!(changes.len(), 2);
        assert_eq!((changes[0].team, changes[0].new_call), (1, Some(1)));
        assert_eq!((changes[1].team, changes[1].new_call), (2, Some(2)));
        // Independent means each carries its own previous value, not a shared one.
        assert_eq!(changes[0].previous, LastSeenCall::Seen(Some(0)));
        assert_eq!(changes[1].previous, LastSeenCall::Seen(Some(0)));
    }

    #[test]
    fn reported_gate_state_matches_the_countdown_at_observation_time() {
        let seen = [LastSeenCall::Seen(Some(0)), LastSeenCall::Seen(Some(0))];

        let pre_gate = detect_call_changes(seen, [Some(1), Some(0)], false);
        assert!(!pre_gate[0].gates_opened);

        let post_gate = detect_call_changes(seen, [Some(1), Some(0)], true);
        assert!(post_gate[0].gates_opened);
    }

    #[test]
    fn a_call_cleared_to_none_is_reported_as_a_change() {
        let seen = [LastSeenCall::Seen(Some(1)), LastSeenCall::Seen(Some(0))];
        let changes = detect_call_changes(seen, [None, Some(0)], false);

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].new_call, None);
        assert_eq!(changes[0].previous, LastSeenCall::Seen(Some(1)));

        // ...and clearing an already-cleared call is not a change.
        let cleared = [LastSeenCall::Seen(None), LastSeenCall::Seen(Some(0))];
        assert!(detect_call_changes(cleared, [None, Some(0)], false).is_empty());
    }

    #[test]
    fn context_is_opening_correction_or_switch_in_the_three_situations() {
        // Gates closed, nothing seen yet: the countdown exchange.
        assert_eq!(
            banter_context_for(LastSeenCall::NeverObserved, false),
            BanterContext::Opening
        );
        // Gates closed, a call already existed: the operator corrected it.
        assert_eq!(
            banter_context_for(LastSeenCall::Seen(Some(0)), false),
            BanterContext::Correction
        );
        // Gates open: a mid-fight shout, regardless of what came before.
        assert_eq!(
            banter_context_for(LastSeenCall::Seen(Some(0)), true),
            BanterContext::Switch
        );
        assert_eq!(
            banter_context_for(LastSeenCall::NeverObserved, true),
            BanterContext::Switch
        );
    }

    #[test]
    fn never_observed_is_distinct_from_an_explicitly_cleared_call() {
        // The sentinel's whole purpose: `Seen(None)` and `NeverObserved` are
        // both "no slot", but only one of them counts as a change against None.
        assert!(LastSeenCall::NeverObserved.differs_from(None));
        assert!(!LastSeenCall::Seen(None).differs_from(None));
        assert_eq!(LastSeenCall::NeverObserved.slot(), None);
        assert_eq!(LastSeenCall::Seen(None).slot(), None);
    }

    #[test]
    fn take_pending_drains_the_queue() {
        let mut watcher = CallWatcher::default();
        watcher.pending = detect_call_changes(FRESH, [Some(0), Some(0)], false);

        let drained = watcher.take_pending();
        assert_eq!(drained.len(), 2);
        assert!(watcher.pending.is_empty(), "a second consumer sees nothing");
        assert!(watcher.take_pending().is_empty());
    }

    // -------------------------------------------------------------------
    // Resource plumbing — the system around the pure function. These build a
    // minimal `World` rather than a full app: the system takes three resources
    // and no queries, so nothing else is needed to exercise it.
    // -------------------------------------------------------------------

    fn run_watcher(world: &mut World) {
        let mut system = IntoSystem::into_system(watch_kill_target_calls);
        system.initialize(world);
        system.run((), world);
    }

    fn world_with(kill_targets: (Option<usize>, Option<usize>), gates_opened: bool) -> World {
        let mut world = World::new();
        let mut config = MatchConfig::default();
        config.team1_kill_target = kill_targets.0;
        config.team2_kill_target = kill_targets.1;
        world.insert_resource(config);
        world.insert_resource(MatchCountdown {
            time_remaining: if gates_opened { 0.0 } else { 10.0 },
            gates_opened,
        });
        world.insert_resource(CallWatcher::default());
        world
    }

    #[test]
    fn the_system_queues_the_opening_change_then_goes_quiet() {
        let mut world = world_with((Some(0), Some(0)), false);

        run_watcher(&mut world);
        assert_eq!(world.resource::<CallWatcher>().pending.len(), 2);

        // Second frame with nothing moved must add nothing.
        run_watcher(&mut world);
        assert_eq!(world.resource::<CallWatcher>().pending.len(), 2);
        assert_eq!(
            world.resource::<CallWatcher>().last_seen,
            [LastSeenCall::Seen(Some(0)), LastSeenCall::Seen(Some(0))]
        );
    }

    #[test]
    fn the_system_picks_up_a_mid_fight_write_to_match_config() {
        let mut world = world_with((Some(0), Some(0)), true);
        run_watcher(&mut world);
        world.resource_mut::<CallWatcher>().take_pending();

        // The in-match control's write: one team only.
        world.resource_mut::<MatchConfig>().team1_kill_target = Some(1);
        run_watcher(&mut world);

        let pending = world.resource_mut::<CallWatcher>().take_pending();
        assert_eq!(pending.len(), 1, "a ResMut deref must not flag team 2");
        assert_eq!(pending[0].team, 1);
        assert_eq!(pending[0].context, BanterContext::Switch);
        assert!(pending[0].gates_opened);
    }

    #[test]
    fn the_system_is_inert_without_a_countdown() {
        // No `MatchCountdown` yet: observing now would burn the first-observation
        // change against an unknown gate state, so the system must skip entirely.
        let mut world = World::new();
        world.insert_resource(MatchConfig::default());
        world.insert_resource(CallWatcher::default());

        run_watcher(&mut world);

        let watcher = world.resource::<CallWatcher>();
        assert!(watcher.pending.is_empty());
        assert_eq!(watcher.last_seen, FRESH, "nothing observed, nothing recorded");
    }

    #[test]
    fn reset_clears_last_seen_and_pending_so_state_cannot_leak_between_matches() {
        let mut world = world_with((Some(0), Some(0)), false);
        run_watcher(&mut world);
        assert!(!world.resource::<CallWatcher>().pending.is_empty());

        let mut system = IntoSystem::into_system(reset_call_watcher_on_exit);
        system.initialize(&mut world);
        system.run((), &mut world);

        let watcher = world.resource::<CallWatcher>();
        assert_eq!(watcher.last_seen, FRESH);
        assert!(watcher.pending.is_empty());

        // The next match therefore reports its own Opening change.
        run_watcher(&mut world);
        let pending = &world.resource::<CallWatcher>().pending;
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|c| c.context == BanterContext::Opening));
    }
}
