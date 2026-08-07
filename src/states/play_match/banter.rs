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
