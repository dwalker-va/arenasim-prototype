---
status: pending
priority: p3
issue_id: "015"
tags: [code-review, defensive, combat]
dependencies: ["012"]
---

# Non-Damage Death Path Missing is_dead Guard

## Problem Statement

At `combat_core.rs:1744`, the non-damage ability death check does not check or set `is_dead`. Currently dead-letter code (no abilities kill without damage), but a defensive gap.

## Proposed Solution

```rust
if !target.is_alive() && !def.is_damage() && !target.is_dead {
    target.is_dead = true;
    // ... log death
}
```

## Work Log

- 2026-02-20: Found during code review
