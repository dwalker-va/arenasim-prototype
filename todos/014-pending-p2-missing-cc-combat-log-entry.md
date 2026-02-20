---
status: pending
priority: p2
issue_id: "014"
tags: [code-review, missing-feature, combat-log]
dependencies: []
---

# CC Interruption Missing [CC] Combat Log Entry

## Problem Statement

The plan specified that CC interruptions should log via `CombatLogEventType::CrowdControl` to produce a `[CC]` entry in the text combat log. The implementation only calls `mark_cast_interrupted()` (for timeline UI) but omits the `combat_log.log()` call.

## Findings

- `combat_core.rs:1243-1248` (process_casting) — only calls `mark_cast_interrupted`
- `combat_core.rs:1870-1875` (process_channeling) — same omission
- Plan acceptance criterion: "Combat log shows [CC] entry when CC interrupts a cast" — not satisfied

## Proposed Solution

Add after `mark_cast_interrupted`:
```rust
combat_log.log(
    CombatLogEventType::CrowdControl,
    format!("{}'s {} interrupted by crowd control", caster_id, ability_def.name),
);
```

## Acceptance Criteria

- [ ] CC-interrupted casts produce a [CC] entry in combat log
- [ ] CC-interrupted channels produce a [CC] entry in combat log

## Work Log

- 2026-02-20: Found during code review
