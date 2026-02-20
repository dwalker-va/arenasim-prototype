---
status: pending
priority: p1
issue_id: "001"
tags: [code-review, security, paladin]
dependencies: []
---

# Division by Zero in HP Percent Calculation

## Problem Statement

The Paladin AI calculates `hp_percent` by dividing `hp / max_hp` without guarding against `max_hp` being zero. If a combatant somehow has 0 max HP, this produces `NaN` or `Infinity`, which propagates through HP threshold checks and causes undefined targeting behavior.

This is a **security/stability issue** that could cause panics or unexpected AI behavior.

## Findings

**Location**: `src/states/play_match/class_ai/paladin.rs:91-95`

```rust
let allies: Vec<AllyInfo> = combatant_info
    .iter()
    .filter(|(_, (team, _, _, hp, _, _))| *team == combatant.team && *hp > 0.0)
    .filter_map(|(e, (_, _, class, hp, max_hp, _))| {
        positions.get(e).map(|pos| AllyInfo {
            entity: *e,
            class: *class,
            hp_percent: *hp / *max_hp,  // <-- Division without guard
            pos: *pos,
        })
    })
    .collect();
```

**Related Issue**: The `.unwrap()` on `partial_cmp` at lines 265, 332, 408 will panic if `hp_percent` is `NaN`:
```rust
.min_by(|a, b| a.hp_percent.partial_cmp(&b.hp_percent).unwrap());
```

## Proposed Solutions

### Option A: Guard Clause (Recommended)
Add a filter to skip combatants with invalid max_hp.

**Pros**: Simple, explicit, follows defensive programming
**Cons**: Silently skips invalid combatants
**Effort**: Small (5 minutes)
**Risk**: Low

```rust
.filter(|(_, (team, _, _, hp, max_hp, _))| *team == combatant.team && *hp > 0.0 && *max_hp > 0.0)
```

### Option B: Safe Division Helper
Create a helper function for safe HP percent calculation.

**Pros**: Reusable, documents intent
**Cons**: More code
**Effort**: Small (15 minutes)
**Risk**: Low

```rust
fn safe_hp_percent(hp: f32, max_hp: f32) -> f32 {
    if max_hp <= 0.0 { 0.0 } else { hp / max_hp }
}
```

### Option C: Fix partial_cmp unwrap
Replace `.unwrap()` with safe default.

**Pros**: Prevents panic even with NaN
**Cons**: Doesn't fix root cause
**Effort**: Small (5 minutes)
**Risk**: Low

```rust
.min_by(|a, b| a.hp_percent.partial_cmp(&b.hp_percent).unwrap_or(std::cmp::Ordering::Equal))
```

## Recommended Action

Implement **Option A + Option C** together - guard against invalid max_hp AND handle NaN in comparisons.

## Technical Details

**Affected Files**:
- `src/states/play_match/class_ai/paladin.rs`

**Lines to Modify**: 91, 104, 265, 332, 408

## Acceptance Criteria

- [ ] No division by zero possible in HP percent calculation
- [ ] No panic possible from NaN in partial_cmp
- [ ] Headless match simulation passes with edge case configs
- [ ] Build succeeds with no new warnings

## Work Log

| Date | Action | Notes |
|------|--------|-------|
| 2026-02-02 | Created | From code review of feat/paladin-class branch |

## Resources

- Security review findings from security-sentinel agent
- PR branch: feat/paladin-class
