---
title: "ResMut Deref Marks a Resource Changed: Why is_changed() Cannot Diff One"
date: 2026-08-08
category: implementation-patterns
module: states/play_match/banter
problem_type: architecture_pattern
severity: medium
applies_when:
  - "A system must react to a specific field of a resource changing value, not merely to the resource being touched"
  - "Any writer of that resource takes `ResMut<T>` — including a writer that usually writes nothing"
  - "Firing on a non-change would be user-visible (a repeated announcement, a re-triggered animation, a duplicated log line)"
tags:
  - bevy-ecs
  - change-detection
  - resources
  - banter
  - ui
---

# ResMut Deref Marks a Resource Changed: Why is_changed() Cannot Diff One

## Context

The in-match kill-target call (PR dwalker-va/arenasim-prototype#98) needed a system that fires **when a team's call moves** — to speak a banter line and write a combat-log entry. The obvious tool is Bevy's built-in change detection:

```rust
// WRONG — fires nearly every frame
fn watch_kill_target_calls(config: Res<MatchConfig>, /* ... */) {
    if !config.is_changed() { return; }
    // ...announce the new call
}
```

This fires constantly. `MatchConfig` is written by `render_team_frames` (`src/states/play_match/rendering/team_frames.rs:774`), which takes `mut config: ResMut<MatchConfig>` because a frame click may set a call. **Bevy's change detection triggers on `DerefMut`, not on a value actually differing** — so the mere existence of a `ResMut` writer that runs every frame is enough to mark the resource changed every frame, whether or not anyone clicked anything.

The failure is not subtle once it fires: an announcement mechanism that re-announces every frame.

## Guidance

When you need "this value became different", keep the previous value yourself and compare. Do not use `is_changed()`.

```rust
/// What the watcher last saw for one team's call.
pub(super) enum LastSeenCall {
    /// No observation has been made for this team yet this match.
    #[default]
    NeverObserved,
    /// The call as of the last observation (`None` = the call was cleared).
    Seen(Option<usize>),
}
```

(`src/states/play_match/banter/watcher.rs:31`)

Two details that made the explicit diff *better* than change detection rather than merely a workaround:

1. **The sentinel buys the match-start case for free.** `NeverObserved` differs from everything, including `Seen(None)`. So the first frame of a match reports a change for both teams, and the opening banter needs no separate match-start code path — it is just the first diff.
2. **Reset the watcher on state exit.** A `Local` or a resource holding last-seen state survives across matches unless something clears it (`reset_call_watcher_on_exit`). Otherwise match two starts already having "seen" match one's final call, and the opening never fires.

`is_changed()` remains correct for its actual purpose: cheap work-skipping where a false positive costs only wasted cycles (rebuilding a cache, re-laying-out a UI). It is wrong wherever a false positive is *observable*.

## Why This Matters

The trap is invisible at the call site. Nothing about `if config.is_changed()` hints that the answer depends on which *other* systems hold a `ResMut` on that resource — a property of the whole schedule, not of this function. It also degrades in the worst direction as a codebase grows: the check works fine until someone adds a `ResMut` writer elsewhere, and then a distant, unrelated commit turns a correct system into one that fires every frame.

Bevy documents this (`DerefMut` triggers detection), and `set_if_neq` exists for the writer-side half of the problem. But `set_if_neq` only helps when the writer assigns a whole value it can compare; it does nothing for a writer that takes `ResMut` and conditionally mutates a field, which is the common UI shape.

## When to Apply

Reach for an explicit last-seen value whenever the reaction is **observable** — speech, logging, sound, an animation trigger, a network message, anything that a user or a test would notice happening twice. Keep `is_changed()` for pure optimization, where re-doing the work is merely wasteful.

A quick test for which you have: *if this fired on a frame where nothing actually changed, would anyone be able to tell?* If yes, diff explicitly.

## Examples

The watcher's diff, sentinel included (`src/states/play_match/banter/watcher.rs:44`):

```rust
fn differs_from(&self, current: Option<usize>) -> bool {
    match self {
        LastSeenCall::NeverObserved => true,
        LastSeenCall::Seen(previous) => *previous != current,
    }
}
```

Related: [Graphical-mode missing system registration](graphical-mode-missing-system-registration.md) is the other "correct in one place, silently wrong in another" trap in this codebase's Bevy usage.
