//! Pre-match and in-fight team banter (graphical only).
//!
//! The team speaks whenever its kill-target call changes — an exchange during
//! the countdown, a corrective beat if the call changes before the gates, and
//! a single-beat shout when it changes mid-fight. Match start counts as a
//! change from nothing, so the opening exchange needs no separate path.
//!
//! Everything here is GRAPHICAL ONLY and registered in `src/states/mod.rs`,
//! never in `add_core_combat_systems`. Line selection and beat timing use a
//! `drip_jitter`-style hash seeded from `GameRng::seed` (read, never drawn
//! from), so no headless baseline can move. See the plan's KTD4-KTD10 in
//! `docs/plans/2026-08-06-001-feat-in-match-kill-call-and-banter-plan.md`.
//!
//! The pipeline runs left to right across the three children:
//!
//!  - [`watcher`]   — detects THAT a call moved and which pool that implies.
//!  - [`resolver`]  — decides WHAT gets said: a pure filter/weight/pick/bind/
//!                    substitute over plain data, no `World` in sight.
//!  - [`scheduler`] — owns the Bevy plumbing: drains the watcher, calls the
//!                    resolver, paces the beats and spawns the bubbles.

mod resolver;
mod scheduler;
pub mod vocab;
mod watcher;

// Re-exported ITEM BY ITEM rather than with the `pub use child::*;` globs
// `combat_core` uses, because `play_match/mod.rs` re-exports this module with a
// glob of its own: anything public here is public across the whole crate. These
// six — two resources and the four systems `StatesPlugin::build()` registers —
// are the entire intended surface, and a deliberate earlier pass demoted
// everything else. An explicit list makes widening it a visible edit.
pub use scheduler::{play_banter_beats, reset_banter_scheduler_on_exit, BanterScheduler};
pub use watcher::{reset_call_watcher_on_exit, watch_kill_target_calls, CallWatcher};

/// Test fixtures shared by more than one child's suite.
///
/// The resolver and scheduler suites both build hand-rolled `BanterExchange`s
/// rather than loading the shipped `banter.ron` — a content edit must never be
/// able to fail a logic test. These three constructors are the overlap, so they
/// live in the parent instead of being duplicated into both children.
#[cfg(test)]
mod test_fixtures {
    use super::super::banter_config::{
        BanterBeat, BanterContext, BanterExchange, BanterSpeaker, ClassConstraint,
    };

    pub(super) fn speaker(role: &str, class: ClassConstraint) -> BanterSpeaker {
        BanterSpeaker { role: role.to_string(), class }
    }

    pub(super) fn beat(role: &str, text: &str) -> BanterBeat {
        BanterBeat { role: role.to_string(), text: text.to_string() }
    }

    /// A two-speaker exchange whose beats are tagged with `label`, so a test can
    /// tell which pool entry the resolver picked by reading the rendered text.
    pub(super) fn two_speaker(
        context: BanterContext,
        label: &str,
        responder: ClassConstraint,
        target: ClassConstraint,
    ) -> BanterExchange {
        BanterExchange {
            context,
            speakers: vec![
                speaker("caller", ClassConstraint::Any),
                speaker("responder", responder),
            ],
            target,
            beats: vec![
                beat("caller", &format!("{}: kill the {{target}}.", label)),
                beat("responder", &format!("{}: on it.", label)),
            ],
        }
    }
}
