---
status: pending
priority: p2
issue_id: "004"
tags: [code-review, performance, paladin]
dependencies: []
---

# Vec Allocations in Hot Path (decide_paladin_action)

## Problem Statement

The `decide_paladin_action()` function allocates two `Vec`s per Paladin per frame to store pre-computed ally and enemy information. While acceptable at current scale (3v3), this creates unnecessary heap allocations that could be avoided with `SmallVec`.

## Findings

**Location**: `src/states/play_match/class_ai/paladin.rs:84-110`

```rust
let allies: Vec<AllyInfo> = combatant_info
    .iter()
    .filter(...)
    .filter_map(...)
    .collect();

let enemies: Vec<EnemyInfo> = combatant_info
    .iter()
    .filter(...)
    .filter_map(...)
    .collect();
```

**Impact Analysis**:
- Current (6 combatants): ~4 heap allocations per frame for 2 Paladins
- At 10x scale: ~40 allocations per frame
- Arena matches are capped at 3v3, so SmallVec<[T; 3]> would eliminate all heap allocations

**Note**: `SmallVec` is already used elsewhere in the codebase (`auras.rs:367`).

## Proposed Solutions

### Option A: Use SmallVec (Recommended)
Replace `Vec` with `SmallVec<[T; 3]>` since team size is bounded at 3.

**Pros**: Zero heap allocations for standard case
**Cons**: Minor code change
**Effort**: Small (15 minutes)
**Risk**: Low

```rust
use smallvec::SmallVec;

let allies: SmallVec<[AllyInfo; 3]> = combatant_info
    .iter()
    // ... same logic
    .collect();
```

### Option B: Remove Pre-computation Entirely
Iterate combatant_info directly in each function (see related todo #002).

**Pros**: No allocations at all, matches other class patterns
**Cons**: Slightly more iteration
**Effort**: Medium (1-2 hours)
**Risk**: Low

### Option C: Keep Current Implementation
Accept the allocations as acceptable overhead.

**Pros**: No changes needed
**Cons**: Suboptimal performance pattern
**Effort**: None
**Risk**: None

## Recommended Action

If keeping the AllyInfo/EnemyInfo pattern (contrary to todo #002), use **Option A**. Otherwise, this is resolved by **Option B** (todo #002).

## Technical Details

**Affected Files**:
- `src/states/play_match/class_ai/paladin.rs`

**Lines to Modify**: 84-110

## Acceptance Criteria

- [ ] No heap allocations for ally/enemy lists in standard 3v3 matches
- [ ] Build succeeds
- [ ] Headless tests pass
- [ ] No performance regression

## Work Log

| Date | Action | Notes |
|------|--------|-------|
| 2026-02-02 | Created | From performance-oracle review |

## Resources

- Performance review findings
- Existing SmallVec usage: `src/states/play_match/auras.rs:367`
