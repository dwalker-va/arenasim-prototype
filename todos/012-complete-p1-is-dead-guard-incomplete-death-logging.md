---
status: pending
priority: p1
issue_id: "012"
tags: [code-review, bugfix, combat]
dependencies: []
---

# is_dead Guard Does Not Prevent Duplicate Death Logging

## Problem Statement

The `is_dead` flag is correctly set at all damage sites, but at 5 of 7+ sites the `log_death` call is in a separate `if is_killing_blow` block that does NOT check `!target.is_dead`. This means a target killed by one system can still have its death logged again by another system in the same frame — the exact bug (BUG-3) this PR aims to fix.

## Findings

At these sites, `is_dead = true` is set inside `if is_killing_blow && !target.is_dead`, but `log_death` is called inside a separate `if is_killing_blow` block below:

| Site | File | Flag set (line) | Death logged (line) | Guarded? |
|------|------|-----------------|---------------------|----------|
| Instant attacks | combat_ai.rs | 639 | 678-689 | NO |
| Frost Nova | combat_ai.rs | 777 | 814-824 | NO |
| Projectile hits | projectiles.rs | 212 | 303-313 | NO |
| Holy Shock | holy_shock.rs | 212 | 247-254 | NO |
| DoT ticks | auras.rs | 738 | 774-780 | NO |
| Channeling | combat_core.rs | 2035 | 2035 (same block) | YES |
| Cast completion | combat_core.rs | 1501 | 1530+ | Partial |
| Auto-attack | combat_core.rs | 883 | 886+ | NO (uses died_this_frame) |

## Proposed Solution

Move `log_death` inside the `!target.is_dead` guard at each site:

```rust
// Before (broken):
let is_killing_blow = !target.is_alive();
if is_killing_blow && !target.is_dead {
    target.is_dead = true;
}
// ... lines later ...
if is_killing_blow {
    combat_log.log_death(...);  // fires even if already dead!
}

// After (fixed):
let is_killing_blow = !target.is_alive();
if is_killing_blow && !target.is_dead {
    target.is_dead = true;
    combat_log.log_death(...);  // only fires once
}
```

## Acceptance Criteria

- [ ] All damage sites gate `log_death` on `!target.is_dead`
- [ ] Run seed 202 and confirm no duplicate death entries
- [ ] Run 5 random matches and confirm single death per combatant

## Work Log

- 2026-02-20: Found during code review by pattern-recognition-specialist and code-simplicity-reviewer agents
