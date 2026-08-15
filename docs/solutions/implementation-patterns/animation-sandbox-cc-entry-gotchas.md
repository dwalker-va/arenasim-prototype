---
title: "Animation Sandbox: Staging Gotchas for CC and Status Entries"
date: 2026-08-12
category: implementation-patterns
module: states/animation_sandbox
problem_type: developer_experience
severity: medium
applies_when:
  - "Adding or reviewing a sandbox entry whose subject is the DUMMY's state (a CC, debuff, or transformation) rather than the caster's motion"
  - "A sandbox entry works on the first pass but degrades or stops across loops"
symptoms:
  - "A looping CC entry silently stops applying after two or three passes"
  - "A distance-driven animation shows nothing in the sandbox"
  - "The interesting part of an entry is over before it can be seen"
tags:
  - animation-sandbox
  - polymorph
  - diminishing-returns
  - movement
  - staging
  - dev-loop
---

# Animation Sandbox: Staging Gotchas for CC and Status Entries

## Context

The animation sandbox substitutes for the AI decision layer only — combat *resolution* (casting, auras, projectiles) runs, but nothing decides actions. Three gotchas bit the Polymorph entry (PR dwalker-va/arenasim-prototype#103); any future CC/status entry (Fear is next) hits the same set.

## Guidance

**1. Nothing moves in the sandbox on its own.** Sim movement (`move_to_target`, including the fear/polymorph wander) is registered under the AI-*decision* gate — `move_to_target.run_if(decide)` in `src/states/play_match/systems.rs`, where graphical mode passes `in_state(GameState::PlayMatch)` — not the shared scene condition (`in_combat_scene`, defined and applied in `src/states/mod.rs` at lines 63 and 223) that admits resolution systems into the sandbox. So a distance-driven animation (walk bob, hop) shows nothing unless the sandbox *stages* the motion: `position_caster` in `src/states/animation_sandbox/playback.rs` owns all staged-unit transforms and walks a unit in a small circle via `circle_walk` (the WalkBob branch for the caster; the Polymorph branch for the dummy). Note the sandbox module doc's `add_sandbox_combat_systems` reference is a stale symbol — no such function exists; the real gating is the `in_combat_scene` scene condition.

**2. A subject that only exists after cast resolution needs an entry-duration hold.** `entry_duration` defaults to the ability's `cast_time` plus the loop tail (~0.6s) — for Polymorph that meant the sheep existed for 0.6s per pass. `POLYMORPH_HOLD_SECS` (4.0) extends the pass so the post-cast state is actually watchable. Any entry whose payoff is an applied aura needs the same treatment.

**3. Looping CC escalates diminishing returns to immunity.** Each CC application advances the target's DR level (full → 50% → 25% → immune) and re-arms the 15s decay timer (`DRTracker::apply`, `src/states/play_match/components/auras.rs`). A looping entry that re-applies every ~6s never lets the timer expire, so by the third pass the dummy is immune and the entry silently stops doing anything. `sustain_staged_units` resets the staged units' `DRTracker` every frame (`DRTracker::reset`), alongside the existing health/mana/stealth sustain — the same sandbox-only rationale: keep entries replayable without touching sim code.

## Why This Matters

All three failures are silent: the entry just looks broken (or worse, looks *fine on the first pass* and degrades), sending the investigation toward the animation code when the problem is staging. The DR one in particular presents as "the animation randomly stopped working."

## When to Apply

- Adding any sandbox entry for a CC, debuff, transformation, or DoT — check all three: does the subject need motion, a post-cast hold, and DR/stack reset?
- Debugging a sandbox entry that works once but not on loop

## Examples

The Polymorph entry end-to-end in `src/states/animation_sandbox/playback.rs`: `circle_walk` dummy staging in `position_caster`, `POLYMORPH_HOLD_SECS` in `entry_duration`, and the `DRTracker` reset in `sustain_staged_units`.

## Related

- `signature-ability-animation-procedure.md` — the sandbox is step 8 of that procedure
- `graphical-mode-missing-system-registration.md` — the dual-registration architecture behind why movement is decision-gated
